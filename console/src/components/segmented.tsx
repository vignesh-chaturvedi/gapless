import { useId } from "react";

import { cn } from "@/lib/utils";

interface SegmentedProps<T extends string | number> {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  className?: string;
}

/** A compact single-choice control. Native radios underneath, so arrow keys and forms work. */
export function Segmented<T extends string | number>({ label, value, options, onChange, className }: SegmentedProps<T>) {
  const name = useId();
  return (
    <fieldset className={cn("min-w-0", className)}>
      <legend className="sr-only">{label}</legend>
      <div className="inline-flex rounded-md border border-hairline bg-background p-0.5">
        {options.map((option) => {
          const id = `${name}-${option.value}`;
          const checked = option.value === value;
          return (
            <label
              key={option.value}
              htmlFor={id}
              className={cn(
                "num relative flex h-6 min-w-10 cursor-pointer items-center justify-center rounded-[5px] px-2 text-xs text-muted-foreground transition-colors duration-100 select-none hover:text-foreground has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring pointer-coarse:h-9",
                checked && "bg-raised text-foreground shadow-[inset_0_0_0_1px_var(--border)]",
              )}
            >
              <input
                id={id}
                type="radio"
                name={name}
                value={String(option.value)}
                checked={checked}
                onChange={() => onChange(option.value)}
                className="sr-only"
              />
              {option.label}
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}
