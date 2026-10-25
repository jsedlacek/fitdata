//! The Google Cloud services fitdata uses: Cloud KMS, Secret Manager and
//! Cloud Storage.
//!
//! The clients are compiled here, once, not with every change to fitdata:
//! each one instantiates every RPC of its service (through its dyn stub), so
//! they are most of the code. That is why the async methods return a boxed
//! future instead of being `async fn`: an `async fn` body is compiled in the
//! crate that awaits it, a boxed future's in the crate that boxes it.

use std::pin::Pin;

pub mod kms;
pub mod secrets;
pub mod store;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
