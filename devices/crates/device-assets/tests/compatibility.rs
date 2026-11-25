//! The historical assets paths and the extracted core/driver paths are identical.
use device_assets::{Memory, NorError, ReadAt, ReadOnlyNor};

fn old_source_is_core_source<T: ReadAt>(source: &mut T) -> Result<(), T::Error> {
    fn core_read<T: device_core::ReadAt>(source: &mut T) -> Result<(), T::Error> {
        device_core::ReadAt::read_exact(source, 1, &mut [0; 2])
    }
    core_read(source)
}

#[test]
fn memory_implements_both_public_trait_paths() {
    let mut memory = Memory::new(&[1, 2, 3]).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(old_source_is_core_source(&mut memory).is_ok());
}

fn same_nor_type<S>(driver: device_spi_nor::ReadOnlyNor<S>) -> ReadOnlyNor<S> {
    driver
}
fn same_error<E>(error: device_spi_nor::NorError<E>) -> NorError<E> {
    error
}

#[test]
fn historical_nor_and_error_exports_keep_type_identity() {
    let _ = same_nor_type::<()>;
    assert_eq!(
        same_error::<()>(device_spi_nor::NorError::Chunk),
        NorError::Chunk
    );
}
