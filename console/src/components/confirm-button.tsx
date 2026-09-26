import { useEffect, useRef, useState, type ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const ARMED_MS = 4_000;

interface ConfirmButtonProps {
  children: ReactNode;
  /** Label once armed, such as "Confirm kill". */
  confirm: string;
  onConfirm: () => void;
  disabled?: boolean;
  /** Why the button is disabled, for the tooltip and screen readers. */
  disabledReason?: string;
  className?: string;
}

/**
 * A destructive action that needs two presses. The first press arms it for four seconds (a
 * draining underline shows the window); the second runs it. Escape, blur or the timeout disarm it.
 */
export function ConfirmButton({ children, confirm, onConfirm, disabled, disabledReason, className }: ConfirmButtonProps) {
  const [primed, setArmed] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  // A button that gets disabled while armed (the stream dropped) stops being armed.
  const armed = primed && !disabled;

  useEffect(() => () => clearTimeout(timer.current), []);

  const disarm = () => {
    clearTimeout(timer.current);
    setArmed(false);
  };

  const press = () => {
    if (!armed) {
      setArmed(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setArmed(false), ARMED_MS);
      return;
    }
    disarm();
    onConfirm();
  };

  return (
    <>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={disabled}
        title={disabled ? disabledReason : undefined}
        onClick={press}
        onBlur={disarm}
        onKeyDown={(event) => {
          if (event.key === "Escape" && armed) {
            event.stopPropagation();
            disarm();
          }
        }}
        className={cn(
          "relative min-w-24 overflow-hidden pointer-coarse:h-10",
          armed && "border-gap/60 bg-gap/10 text-gap hover:bg-gap/15 hover:text-gap dark:border-gap/60 dark:bg-gap/15",
          className,
        )}
      >
        {armed ? confirm : children}
        {armed && (
          <span
            aria-hidden="true"
            className="absolute inset-x-0 bottom-0 h-0.5 origin-left bg-gap motion-safe:animate-[drain_4s_linear_forwards]"
          />
        )}
      </Button>
      <span role="status" aria-live="polite" className="sr-only">
        {armed ? "Press again to confirm, or Escape to cancel." : ""}
      </span>
    </>
  );
}
