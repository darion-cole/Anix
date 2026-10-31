/** The system Anix runs on, for the few places the macOS build behaves differently. */

/** True in the macOS build (its web view reports a Mac user agent). */
export function isMacOS(): boolean {
  return typeof navigator !== 'undefined' && /Macintosh|Mac OS X/.test(navigator.userAgent);
}
