import { cn } from "@/lib/utils";

const DIGITS = "0123456789";

/**
 * A formatted number whose digits roll to new values, odometer-style. Only digits move; commas,
 * units and dashes render as they are. Digits are keyed from the right, so a number that grows a
 * digit doesn't reshuffle the others. Each digit keeps an invisible copy in the flow, so width
 * and baseline match plain text (clip-path, unlike overflow, doesn't move the baseline).
 *
 * Everything drawn is CSS generated content (the 0–9 strips, commas, units), so it's never
 * selected, copied or found; a transparent copy of the value on top is what selection, copy and
 * screen readers get.
 * Reduced motion turns the roll into a swap through the global transition rule.
 */
export function RollingNumber({ value, className }: { value: string; className?: string }) {
  const chars = [...value];
  return (
    <span className={cn("num relative inline-block whitespace-nowrap", className)}>
      <span aria-hidden="true" className="select-none">
        {chars.map((char, i) => {
          const key = chars.length - i;
          const digit = DIGITS.indexOf(char);
          if (digit < 0) {
            return <span key={`c${key}${char}`} data-char={char} className="before:content-[attr(data-char)]" />;
          }
          return (
            <span key={`d${key}`} className="relative inline-block [clip-path:inset(0_-0.05em)]">
              <span className="invisible">0</span>
              <span
                className="digit-strip absolute inset-x-0 top-0 transition-transform duration-500 ease-crisp"
                style={{ transform: `translateY(${-digit * 10}%)` }}
              />
            </span>
          );
        })}
      </span>
      <span className="absolute inset-0 text-transparent">{value}</span>
    </span>
  );
}
