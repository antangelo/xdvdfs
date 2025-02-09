#[cfg(feature = "sync")]
macro_rules! run_with_executor {
    ($f:ident, $($x:expr),*) => {
        $f($($x),*)
    };
}

#[cfg(not(feature = "sync"))]
macro_rules! run_with_executor {
    ($f:ident, $($x:expr),*) => {
        futures::executor::block_on($f($($x),*))
    };
}

pub(crate) use run_with_executor;
