/// It'd be nice to debounce WM_NULL spam.  However, PostThreadMessageW is unreliable:
///
/// > Messages sent by PostThreadMessage are not associated with a window. As a general rule, messages that are not
/// > associated with a window cannot be dispatched by the DispatchMessage function. Therefore, if the recipient thread
/// > is in a modal loop (as used by MessageBox or DialogBox), the messages will be lost. To intercept thread messages
/// > while in a modal loop, use a thread-specific hook.
/// >
/// > <https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postthreadmessagew#remarks>
///
/// Currently the code bellow assumes WM_NULL is reliable for [`Spawner::spawn`]?
/// That should instead set a tracking bool?
/// Waker might also need to set a tracking bool *on top of* sending WM_NULL?
const ERROR : () = compile_error!("PostThreadMessageW is unreliable!");



use winapi::um::processthreadsapi::GetCurrentThreadId;
use winapi::um::winuser::{PostThreadMessageW, WM_NULL};

use std::collections::VecDeque;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Mutex, Arc, Weak};
use std::task::{Waker, Context, Poll};



pub struct Pool {
    waker:              Waker,
    executing:          VecDeque<Entry>,
    shared:             Arc<Mutex<Shared>>,
    _not_thread_safe:   PhantomData<*const ()>, // not necessary for soundness... but using `Pool` directly from another thread is a bug
}

#[derive(Clone)]
pub struct Spawner(Weak<Mutex<Shared>>);

#[derive(Default)]
struct Shared {
    thread_id:          u32,
    pending:            VecDeque<Entry>,
}

struct Entry {
    future:             Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    done:               bool,
}



impl Pool {
    pub fn new() -> Self {
        let thread_id = unsafe { GetCurrentThreadId() };

        let waker = waker_fn::waker_fn(move || assert!(unsafe { PostThreadMessageW(thread_id, WM_NULL, 0, 0) } != 0));

        Self {
            waker,
            shared:             Arc::new(Mutex::new(Shared::new(thread_id))),
            executing:          Default::default(),
            _not_thread_safe:   Default::default(),
        }
    }

    pub fn spawner(&self) -> Spawner { Spawner(Arc::downgrade(&self.shared)) }

    pub fn run_until_stalled(&mut self) {
        let mut shared = self.shared.lock().unwrap();
        self.executing.append(&mut shared.pending);
        drop(shared); // unlock

        loop {
            let mut any = false;
            for e in self.executing.iter_mut() {
                match e.future.as_mut().poll(&mut Context::from_waker(&self.waker)) {
                    Poll::Pending => {},
                    Poll::Ready(()) => {
                        e.done = true;
                        any = true;
                    },
                }
            }
            if !any { return }
            self.executing.retain(|e| !e.done);
        }
    }
}

impl Spawner {
    /// Spawns work to execute on the UI thread the [`Pool`] was created on
    pub fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) -> Result<(), ()> {
        let future = Box::pin(future);
        let shared = self.0.upgrade().ok_or(())?;
        let mut shared = shared.lock().map_err(|_| ())?;
        let thread_id = shared.thread_id;
        shared.pending.push_back(Entry { future, done: false });
        drop(shared); // unlock
        assert!(unsafe { PostThreadMessageW(thread_id, WM_NULL, 0, 0) } != 0);
        Ok(())
    }
}

impl Shared {
    fn new(thread_id: u32) -> Self {
        Self { thread_id, pending: Default::default() }
    }
}

impl Default for Pool {
    fn default() -> Self { Self::new() }
}
