import { create } from "zustand";

/** Whether the command palette is open; any button can open it. */
export const useCommandMenu = create<{ open: boolean; setOpen: (open: boolean) => void }>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}));
