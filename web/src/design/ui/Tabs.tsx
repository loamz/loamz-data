"use client";

import * as TabsPrimitive from "@radix-ui/react-tabs";
import { forwardRef, type ComponentPropsWithoutRef, type ElementRef } from "react";
import { cn } from "../cn";

export const Tabs = TabsPrimitive.Root;
export const TabsContent = TabsPrimitive.Content;

/** `pill` (default): a small switch inside a view. `line`: the tabs of an object's page, an underline under the current one. */
export const TabsList = forwardRef<ElementRef<typeof TabsPrimitive.List>, ComponentPropsWithoutRef<typeof TabsPrimitive.List> & { variant?: "pill" | "line" }>(
  ({ className, variant = "pill", ...props }, ref) => (
    <TabsPrimitive.List
      ref={ref}
      data-variant={variant}
      className={cn(
        variant === "line" ? "flex h-9 items-end gap-5 border-b border-border" : "inline-flex h-control-md items-center gap-0.5 rounded-md bg-badge p-0.5",
        className,
      )}
      {...props}
    />
  ),
);
TabsList.displayName = "TabsList";

export const TabsTrigger = forwardRef<ElementRef<typeof TabsPrimitive.Trigger>, ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>>(
  ({ className, ...props }, ref) => (
    <TabsPrimitive.Trigger
      ref={ref}
      className={cn(
        "inline-flex h-full cursor-pointer items-center justify-center rounded-sm px-3 text-xs font-medium text-muted transition-colors " +
          "hover:text-foreground data-[state=active]:bg-card data-[state=active]:text-foreground data-[state=active]:shadow-sm " +
          "in-data-[variant=line]:rounded-none in-data-[variant=line]:border-b-2 in-data-[variant=line]:border-transparent in-data-[variant=line]:px-0 in-data-[variant=line]:text-sm " +
          "in-data-[variant=line]:data-[state=active]:border-accent in-data-[variant=line]:data-[state=active]:bg-transparent in-data-[variant=line]:data-[state=active]:text-accent in-data-[variant=line]:data-[state=active]:shadow-none",
        className,
      )}
      {...props}
    />
  ),
);
TabsTrigger.displayName = "TabsTrigger";
