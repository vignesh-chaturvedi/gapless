import { Activity, BookOpen, Flame, Gauge, History, Moon, Scissors, ShieldCheck, Snail, Sun, Wrench } from "lucide-react";
import { useEffect } from "react";
import { useNavigate } from "react-router";

import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/components/ui/command";
import { SLOW_CONSUMER_MS, cutStream, killStream, setHandoffPatch, setSlowConsumer } from "@/lib/chaos";
import { useCommandMenu } from "@/lib/command-menu";
import { useFeed } from "@/lib/feed";
import { toggleTheme, useTheme } from "@/lib/theme";

export function CommandMenu() {
  const { open, setOpen } = useCommandMenu();
  const navigate = useNavigate();
  const theme = useTheme();
  const controls = useFeed((s) => s.snapshot?.controls);
  const offline = useFeed((s) => s.snapshot?.mode === "offline");

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        setOpen(!useCommandMenu.getState().open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setOpen]);

  const run = (action: () => void) => {
    setOpen(false);
    action();
  };

  return (
    <CommandDialog open={open} onOpenChange={setOpen} title="Commands" description="Jump anywhere or break the stream on purpose">
      <Command>
      <CommandInput placeholder="Search commands…" />
      <CommandList>
        <CommandEmpty>No matching command.</CommandEmpty>
        <CommandGroup heading="Go to">
          <CommandItem onSelect={() => run(() => navigate("/live"))}>
            <Activity aria-hidden="true" />
            Live console
          </CommandItem>
          <CommandItem onSelect={() => run(() => navigate("/incidents"))}>
            <History aria-hidden="true" />
            Incidents
          </CommandItem>
          <CommandItem onSelect={() => run(() => navigate("/verify"))}>
            <ShieldCheck aria-hidden="true" />
            Verification
          </CommandItem>
          <CommandItem onSelect={() => run(() => navigate("/integrate"))}>
            <BookOpen aria-hidden="true" />
            Integrate
          </CommandItem>
          <CommandItem onSelect={() => run(() => navigate("/"))}>
            <Gauge aria-hidden="true" />
            Overview
          </CommandItem>
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Break it on purpose">
          <CommandItem onSelect={() => run(() => killStream(60))}>
            <Flame aria-hidden="true" />
            {offline ? "Cut the stream, stay offline 60s" : "Kill the stream via Solami, stay offline 60s"}
          </CommandItem>
          <CommandItem onSelect={() => run(() => killStream())}>
            <Flame aria-hidden="true" />
            {offline ? "Cut the stream, reconnect at once" : "Kill the stream via Solami, reconnect at once"}
          </CommandItem>
          <CommandItem onSelect={() => run(() => cutStream(30))}>
            <Scissors aria-hidden="true" />
            Cut the connection from our side, stay offline 30s
          </CommandItem>
          <CommandItem onSelect={() => run(() => setSlowConsumer(controls?.throttleMs ? null : SLOW_CONSUMER_MS))}>
            <Snail aria-hidden="true" />
            {controls?.throttleMs ? "Restore full consumer speed" : "Slow the consumer until Solami pushes back"}
          </CommandItem>
          <CommandItem onSelect={() => run(() => setHandoffPatch(!(controls?.handoffPatch ?? true)))}>
            <Wrench aria-hidden="true" />
            {(controls?.handoffPatch ?? true) ? "Turn the handoff patch off" : "Turn the handoff patch on"}
          </CommandItem>
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Appearance">
          <CommandItem onSelect={() => run(toggleTheme)}>
            {theme === "dark" ? <Sun aria-hidden="true" /> : <Moon aria-hidden="true" />}
            {theme === "dark" ? "Light theme" : "Dark theme"}
            <CommandShortcut>⇧⌘L</CommandShortcut>
          </CommandItem>
        </CommandGroup>
      </CommandList>
      </Command>
    </CommandDialog>
  );
}
