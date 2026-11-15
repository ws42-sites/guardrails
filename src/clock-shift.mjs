// Preloaded via NODE_OPTIONS=--import by `guardrails check` (TEST-6).
// Shifts the real clock forward; vi.useFakeTimers()/setSystemTime still win.
const years = Number(process.env.GUARDRAILS_CLOCK_SHIFT_YEARS || 0);
if (years && !globalThis.__guardrailsClockShift) {
  const RealDate = globalThis.Date;
  const offset = Math.round(years * 365.25 * 864e5);
  function ShiftedDate(...args) {
    if (!new.target) return new RealDate(RealDate.now() + offset).toString();
    return args.length ? new RealDate(...args) : new RealDate(RealDate.now() + offset);
  }
  ShiftedDate.prototype = RealDate.prototype;
  ShiftedDate.now = () => RealDate.now() + offset;
  ShiftedDate.parse = RealDate.parse;
  ShiftedDate.UTC = RealDate.UTC;
  Object.defineProperty(ShiftedDate, Symbol.hasInstance, { value: (x) => x instanceof RealDate });
  globalThis.Date = ShiftedDate;
  globalThis.__guardrailsClockShift = offset;
}
