use std::sync::{LazyLock, Mutex};

use malachitebft_core_types::{NilOrVal, Round};

use crate::{Address, Height, ValueId, Vote};

static MIDDLEWARE: LazyLock<Mutex<Box<dyn Middleware>>> =
    LazyLock::new(|| Mutex::new(Box::new(DefaultMiddleware)));

fn set(middleware: impl Middleware + 'static) {
    *MIDDLEWARE.lock().unwrap() = Box::new(middleware);
}

fn reset() {
    *MIDDLEWARE.lock().unwrap() = Box::new(DefaultMiddleware);
}

pub async fn scoped<F, R>(middleware: impl Middleware + 'static, f: F) -> R
where
    F: AsyncFnOnce() -> R,
{
    set(middleware);
    let result = f().await;
    reset();
    result
}

pub fn with<F, R>(f: F) -> R
where
    F: FnOnce(&dyn Middleware) -> R,
{
    let middleware = MIDDLEWARE.lock().unwrap();
    let middleware = middleware.as_ref();
    f(middleware)
}

pub trait Middleware: Send + Sync {
    fn new_prevote(
        &self,
        height: Height,
        round: Round,
        value_id: NilOrVal<ValueId>,
        address: Address,
    ) -> Vote {
        Vote::new_prevote(height, round, value_id, address)
    }

    fn new_precommit(
        &self,
        height: Height,
        round: Round,
        value_id: NilOrVal<ValueId>,
        address: Address,
    ) -> Vote {
        Vote::new_precommit(height, round, value_id, address)
    }
}

pub struct DefaultMiddleware;

impl Middleware for DefaultMiddleware {}
