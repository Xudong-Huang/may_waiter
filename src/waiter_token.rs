
use std::time::Duration;
use std::iter::FromIterator;
use std::sync::atomic::Ordering;
use std::io::{self, Error, ErrorKind};

use base64;
use may::coroutine;
use rand::{Rng, OsRng};
use may::sync::{AtomicOption, Blocker};
use crypto::{symmetriccipher, buffer, aes, blockmodes};
use crypto::buffer::{ReadBuffer, WriteBuffer, BufferResult};

pub struct Waiter<T> {
    blocker: Blocker,
    rsp: AtomicOption<T>,
}

impl<T> Waiter<T> {
    pub fn new() -> Self {
        Waiter {
            blocker: Blocker::new(false),
            rsp: AtomicOption::none(),
        }
    }

    pub fn set_rsp(&self, rsp: T) {
        // set the response
        self.rsp.swap(rsp, Ordering::Release);
        // wake up the blocker
        self.blocker.unpark();
    }

    pub fn wait_rsp<D: Into<Option<Duration>>>(&self, timeout: D) -> io::Result<T> {
        use coroutine::ParkError;
        match self.blocker.park(timeout.into()) {
            Ok(_) => {
                match self.rsp.take(Ordering::Acquire) {
                    Some(rsp) => Ok(rsp),
                    None => panic!("unable to get the rsp, waiter={:p}", &self),
                }
            }
            Err(ParkError::Timeout) => {
                error!("waiter timeout {:p}", &self);
                Err(Error::new(ErrorKind::TimedOut, "wait rsp timeout"))
            }
            Err(ParkError::Canceled) => {
                error!("waiter canceled {:p}", &self);
                coroutine::trigger_cancel_panic();
            }
        }
    }
}

#[derive(Debug)]
pub struct WaiterToken {
    key: [u8; 32],
    iv: [u8; 16],
    nonce: [u8; 16],
}

fn ref_to_bytes<T>(ptr: &T) -> [u8; 8] {
    unsafe { ::std::mem::transmute(ptr) }
}

fn bytes_to_ref<T>(bytes: [u8; 8]) -> *const T {
    unsafe { ::std::mem::transmute(bytes) }
}

// Encrypt a buffer with the given key and iv using
// AES-256/CBC/Pkcs encryption.
fn encrypt(data: &[u8],
           key: &[u8],
           iv: &[u8])
           -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {

    // Create an encryptor instance of the best performing
    // type available for the platform.
    let mut encryptor =
        aes::cbc_encryptor(aes::KeySize::KeySize256, key, iv, blockmodes::PkcsPadding);

    // Each encryption operation encrypts some data from
    // an input buffer into an output buffer. Those buffers
    // must be instances of RefReaderBuffer and RefWriteBuffer
    // (respectively) which keep track of how much data has been
    // read from or written to them.
    let mut final_result = Vec::<u8>::new();
    let mut read_buffer = buffer::RefReadBuffer::new(data);
    let mut buffer = [0; 4096];
    let mut write_buffer = buffer::RefWriteBuffer::new(&mut buffer);

    // Each encryption operation will "make progress". "Making progress"
    // is a bit loosely defined, but basically, at the end of each operation
    // either BufferUnderflow or BufferOverflow will be returned (unless
    // there was an error). If the return value is BufferUnderflow, it means
    // that the operation ended while wanting more input data. If the return
    // value is BufferOverflow, it means that the operation ended because it
    // needed more space to output data. As long as the next call to the encryption
    // operation provides the space that was requested (either more input data
    // or more output space), the operation is guaranteed to get closer to
    // completing the full operation - ie: "make progress".
    //
    // Here, we pass the data to encrypt to the enryptor along with a fixed-size
    // output buffer. The 'true' flag indicates that the end of the data that
    // is to be encrypted is included in the input buffer (which is true, since
    // the input data includes all the data to encrypt). After each call, we copy
    // any output data to our result Vec. If we get a BufferOverflow, we keep
    // going in the loop since it means that there is more work to do. We can
    // complete as soon as we get a BufferUnderflow since the encryptor is telling
    // us that it stopped processing data due to not having any more data in the
    // input buffer.
    loop {
        let result = try!(encryptor.encrypt(&mut read_buffer, &mut write_buffer, true));

        // "write_buffer.take_read_buffer().take_remaining()" means:
        // from the writable buffer, create a new readable buffer which
        // contains all data that has been written, and then access all
        // of that data as a slice.
        final_result.extend(write_buffer
                                .take_read_buffer()
                                .take_remaining()
                                .iter()
                                .map(|&i| i));

        match result {
            BufferResult::BufferUnderflow => break,
            BufferResult::BufferOverflow => {}
        }
    }

    Ok(final_result)
}

// Decrypts a buffer with the given key and iv using
// AES-256/CBC/Pkcs encryption.
//
// This function is very similar to encrypt(), so, please reference
// comments in that function. In non-example code, if desired, it is possible to
// share much of the implementation using closures to hide the operation
// being performed. However, such code would make this example less clear.
fn decrypt(encrypted_data: &[u8],
           key: &[u8],
           iv: &[u8])
           -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
    let mut decryptor =
        aes::cbc_decryptor(aes::KeySize::KeySize256, key, iv, blockmodes::PkcsPadding);

    let mut final_result = Vec::<u8>::new();
    let mut read_buffer = buffer::RefReadBuffer::new(encrypted_data);
    let mut buffer = [0; 4096];
    let mut write_buffer = buffer::RefWriteBuffer::new(&mut buffer);

    loop {
        let result = try!(decryptor.decrypt(&mut read_buffer, &mut write_buffer, true));
        final_result.extend(write_buffer
                                .take_read_buffer()
                                .take_remaining()
                                .iter()
                                .map(|&i| i));
        match result {
            BufferResult::BufferUnderflow => break,
            BufferResult::BufferOverflow => {}
        }
    }

    Ok(final_result)
}

impl WaiterToken {
    pub fn new() -> Self {
        // generate random key and nonce
        let mut rng = OsRng::new().unwrap();
        let mut key = [0u8; 32];
        let mut iv = [0u8; 16];
        let mut nonce = [0u8; 16];
        rng.fill_bytes(&mut key);
        rng.fill_bytes(&mut iv);
        rng.fill_bytes(&mut nonce);
        println!("key={:?}, iv={:?}, nonce={:?}", key, iv, nonce);
        WaiterToken {
            key: key,
            iv: iv,
            nonce: nonce,
        }
    }

    pub fn waiter_to_token<T>(&self, waiter: &Waiter<T>) -> String {
        println!("waiter_s={:p}", waiter);
        //first serial nonce
        let mut data = Vec::from_iter(self.nonce.iter().map(|&i| i));
        // then serial ptr
        data.extend(ref_to_bytes(waiter).iter());

        let encrypt_data = encrypt(&data, &self.key, &self.iv).unwrap();

        base64::encode(&encrypt_data)
    }

    pub fn token_to_waiter<T>(&self, token: &str) -> Option<&Waiter<T>> {
        let raw_data = match base64::decode(token.as_bytes()) {
            Ok(data) => data,
            Err(_) => return None,
        };

        let result = match decrypt(&raw_data, &self.key, &self.iv) {
            Ok(data) => data,
            Err(_) => return None,
        };

        if result.len() != 24 {
            return None;
        }

        let mut data = [0u8; 16];
        data.copy_from_slice(&result[0..16]);
        // need to verify if the nonce is correct
        if data != self.nonce {
            return None;
        }

        let mut data = [0u8; 8];
        data.copy_from_slice(&result[16..]);
        let ptr = bytes_to_ref::<&Waiter<T>>(data);
        let waiter = unsafe { &*(ptr as *const _) };
        println!("waiter={:p}", waiter);
        Some(waiter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_waiter() {
        use std::sync::Arc;
        let req_map = Arc::new(WaiterToken::new());
        let rmap = req_map.clone();

        // one coroutine wait data send from another coroutine
        // prepare the waiter first
        let waiter = Waiter::<usize>::new();
        let token = req_map.waiter_to_token(&waiter);
        println!("token={}", token);
        // trigger the rsp in another coroutine
        coroutine::spawn(move || rmap.token_to_waiter(&token).map(|w| w.set_rsp(100)));

        // this will block until the rsp was set
        let result = waiter.wait_rsp(None).unwrap();
        assert_eq!(result, 100);
    }
}