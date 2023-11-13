use futures::stream::{Stream, StreamExt};

/// Returns a transformed stream that yields items along with a provided stream identifier
pub fn with_id<T: Clone, S: Stream>(id: T, stream: S) -> impl Stream<Item = (T, S::Item)> {
    stream.map(move |item| (id.clone(), item))
}
