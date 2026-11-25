//! Fixed cooperative storage allocation, independent of chip and medium.
/// Advance at most four history phases and one property phase per foreground pass.
/// Each underlying phase still admits or polls at most one medium operation.
/// This is a finite work bound, not a timing guarantee; Host/UI run between passes.
/// # Errors
/// Preserve the first history error while still servicing accepted property work.
#[inline]
pub fn advance_storage<E>(
    mut history: impl FnMut() -> Result<(), E>,
    mut properties: impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    let mut result = Ok(());
    for _ in 0..4 {
        if let Err(error) = history() {
            result = Err(error);
            break;
        }
    }
    let property = properties();
    result.and(property)
}
