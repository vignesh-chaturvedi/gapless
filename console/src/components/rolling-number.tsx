import { cn } from "@/lib/utils";

const DIGITS = [..."0123456789"];

/**
 * A formatted number whose digits roll to new values, odometer-style. Only digits move; commas,
 * units and dashes render as they are. Digits are keyed from the right, so a number that grows a
 * digit doesn't reshuffle the others. Each digit keeps an invisible copy in the flow, so width
 * and baseline match plain text (clip-path, unlike overflow, doesn't move the baseline).
 * Reduced motion turns the roll into a swap through the global transition rule.
 */
export function RollingNumber({ value, className }: { value: string; className?: string }) {
  const chars = [...value];
  return (
    <span className={cn("num whitespace-nowrap", className)}>
      <span aria-hidden="true">
        {chars.map((char, i) => {
          const key = chars.length - i;
          const digit = DIGITS.indexOf(char);
          if (digit < 0) return <span key={`c${key}${char}`}>{char}</span>;
          return (
            <span key={`d${key}`} className="relative inline-block [clip-path:inset(0_-0.05em)]">
              <span className="invisible">0</span>
              <span
                className="absolute inset-x-0 top-0 flex flex-col transition-transform duration-500 ease-crisp"
                style={{ transform: `translateY(${-digit * 10}%)` }}
              >
                {DIGITS.map((d) => (
                  <span key={d}>{d}</span>
                ))}
              </span>
            </span>
          );
        })}
      </span>
      <span className="sr-only">{value}</span>
    </span>
  );
}
