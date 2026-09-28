use std::io::Read;
use std::os::fd::FromRawFd;
use std::sync::Mutex;
use winit::event_loop::{EventLoop, EventLoopBuilder};
use winit::platform::android::activity::AndroidApp;
use winit::platform::android::EventLoopBuilderExtAndroid;

static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);

pub fn remember(app: AndroidApp)
{
    *APP.lock().unwrap() = Some(app);
}

pub fn event_loop() -> EventLoop<()>
{
    let app = APP.lock().unwrap().clone().expect("android app");
    let mut builder = EventLoopBuilder::new();
    builder.with_android_app(app);

    builder.build().unwrap()
}

pub fn redirect_stdio()
{
    unsafe {
        let mut pipes = [0i32; 2];

        if libc::pipe(pipes.as_mut_ptr()) != 0
        {
            return;
        }

        libc::dup2(pipes[1], libc::STDOUT_FILENO);
        libc::dup2(pipes[1], libc::STDERR_FILENO);
        libc::close(pipes[1]);
        let read_fd = pipes[0];

        std::thread::spawn(move || {
            let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
            let mut buf = [0u8; 2048];

            loop
            {
                let count = match file.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                log_chunk(&buf[..count]);
            }
        });
    }
}

fn log_chunk(bytes: &[u8])
{
    let text = String::from_utf8_lossy(bytes);

    for line in text.split('\n')
    {
        if line.is_empty()
        {
            continue;
        }

        let cleaned = line.replace('\0', "");
        let Ok(message) = std::ffi::CString::new(cleaned) else {
            continue;
        };

        unsafe {
            __android_log_write(4, c"engine".as_ptr(), message.as_ptr());
        }
    }
}

unsafe extern "C" {
    fn __android_log_write(priority: i32, tag: *const libc::c_char, text: *const libc::c_char) -> i32;
}
