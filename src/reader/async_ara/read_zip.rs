use crate::{Archive, Error};
use ara::ReadAt;
use async_trait::async_trait;

#[async_trait(?Send)]
pub trait AsyncReadZip {
    async fn read_zip(&self) -> Result<Archive, Error>;
}

#[async_trait(?Send)]
impl<T> AsyncReadZip for T
where
    T: ReadAt,
{
    async fn read_zip(&self) -> Result<Archive, Error> {
        todo!()
    }
}
