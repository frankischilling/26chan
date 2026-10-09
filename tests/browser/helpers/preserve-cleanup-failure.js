// Teardown must remain fatal on success, but must not replace the action or
// assertion that originally failed. The caller records secondary diagnostics.
export async function withFailurePreservingCleanup(body, cleanup, recordSecondary = console.error) {
  let failed = false, failure;
  try {
    return await body();
  } catch (error) {
    failed = true; failure = error;
    throw error;
  } finally {
    try { await cleanup(); }
    catch (error) {
      if (!failed) throw error;
      try { recordSecondary(error); }
      finally { throw failure; }
    }
  }
}
