export function PageLoading() {
  return (
    <div className="page-loading" role="status" aria-label="Opening page">
      <span className="sr-only">Opening page…</span>
      <div aria-hidden="true" className="page-loading-title" />
      <div aria-hidden="true" className="page-loading-line" />
      <div aria-hidden="true" className="page-loading-panel" />
    </div>
  );
}
