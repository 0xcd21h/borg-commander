use std::{io, path::Path};

use futures::stream::BoxStream;

use crate::Entry;

pub type Batches = BoxStream<'static, io::Result<Vec<Entry>>>;

pub trait Backend: Send + Sync {
    fn list(&self, dir: &Path) -> Batches;
}
