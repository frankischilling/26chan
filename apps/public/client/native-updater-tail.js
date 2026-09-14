// The original client compares the age of its tail window with elapsed time
// since the last completed request. Invalid dates choose a complete response.
export function useUpdaterTail(size, replyTimes, lastUpdated, now) {
  if (!Number.isInteger(size) || size <= 0 || size > 1000
    || !Number.isFinite(lastUpdated) || !Number.isFinite(now) || now < lastUpdated) return false;
  const index = replyTimes.length - size;
  if (index < 0) return true;
  const timestamp = replyTimes[index];
  if (!Number.isFinite(timestamp)) return false;
  return Math.floor(lastUpdated / 1000) - Math.floor(timestamp / 1000)
    > Math.floor((now - lastUpdated) / 1000);
}
