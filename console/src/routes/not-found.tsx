import { Link } from "react-router";

import { Button } from "@/components/ui/button";

export function NotFound() {
  return (
    <div className="mx-auto flex max-w-xl flex-col items-start gap-4 px-4 py-20 lg:px-6">
      <p className="label">404</p>
      <h1 className="text-2xl font-semibold tracking-[-0.025em]">This slot was skipped</h1>
      <p className="text-muted-foreground">There's no page here. The live console is a good place to start.</p>
      <Button asChild>
        <Link to="/live">Open the live console</Link>
      </Button>
    </div>
  );
}
