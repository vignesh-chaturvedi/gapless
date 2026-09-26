import { Menu, Moon, Search, Sun } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, NavLink } from "react-router";

import { Wordmark } from "@/components/brand/mark";
import { useCommandMenu } from "@/lib/command-menu";
import { StreamPill } from "@/components/stream-pill";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import { NAV } from "@/lib/nav";
import { toggleTheme, useTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";


const navItem =
  "rounded-md px-2.5 py-1.5 text-sm text-muted-foreground transition-colors duration-100 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-[current=page]:bg-muted aria-[current=page]:text-foreground";

export function ThemeToggle() {
  const theme = useTheme();
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() === "l" && event.shiftKey && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        toggleTheme();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={toggleTheme}
      aria-label={theme === "dark" ? "Switch to the light theme" : "Switch to the dark theme"}
      title="Toggle theme (⇧⌘L)"
      className="pointer-coarse:size-10"
    >
      {theme === "dark" ? <Sun aria-hidden="true" /> : <Moon aria-hidden="true" />}
    </Button>
  );
}

function MobileNav() {
  const [open, setOpen] = useState(false);
  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetTrigger asChild>
        <Button variant="ghost" size="icon" aria-label="Open navigation" className="size-10 md:hidden">
          <Menu aria-hidden="true" />
        </Button>
      </SheetTrigger>
      <SheetContent side="right" className="w-72">
        <SheetHeader>
          <SheetTitle>
            <Wordmark />
          </SheetTitle>
        </SheetHeader>
        <nav aria-label="Console" className="flex flex-col gap-1 px-4">
          {NAV.map((item) => (
            <NavLink key={item.to} to={item.to} onClick={() => setOpen(false)} className={cn(navItem, "py-2.5 text-base")}>
              {item.label}
            </NavLink>
          ))}
        </nav>
      </SheetContent>
    </Sheet>
  );
}

export function TopBar() {
  const setOpen = useCommandMenu((s) => s.setOpen);
  return (
    <header className="sticky top-0 z-40 border-b border-hairline bg-background/85 backdrop-blur-md supports-[backdrop-filter]:bg-background/70">
      <div className="flex h-14 items-center gap-6 px-4 lg:px-6">
        <Link
          to="/"
          aria-label="Gapless overview"
          className="rounded-md focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <Wordmark />
        </Link>
        <nav aria-label="Console" className="hidden items-center gap-1 md:flex">
          {NAV.map((item) => (
            <NavLink key={item.to} to={item.to} className={navItem}>
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="ml-auto flex items-center gap-2">
          <StreamPill />
          <Button
            variant="outline"
            size="sm"
            onClick={() => setOpen(true)}
            className="hidden gap-2 text-muted-foreground sm:inline-flex"
          >
            <Search aria-hidden="true" />
            Commands
            <Kbd>⌘K</Kbd>
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setOpen(true)}
            aria-label="Open commands"
            className="size-10 sm:hidden"
          >
            <Search aria-hidden="true" />
          </Button>
          <ThemeToggle />
          <MobileNav />
        </div>
      </div>
    </header>
  );
}
