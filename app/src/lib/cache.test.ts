import { describe, expect, it, vi } from "vitest";
import { memoizeAsync } from "./cache";

describe("memoizeAsync", () => {
  it("only calls compute once for repeated calls with the same key", async () => {
    const store = new Map<string, Promise<number>>();
    const compute = vi.fn(async () => 42);

    const a = await memoizeAsync(store, "x", compute);
    const b = await memoizeAsync(store, "x", compute);

    expect(a).toBe(42);
    expect(b).toBe(42);
    expect(compute).toHaveBeenCalledTimes(1);
  });

  it("computes independently per key", async () => {
    const store = new Map<string, Promise<string>>();
    const a = await memoizeAsync(store, "a", async () => "A");
    const b = await memoizeAsync(store, "b", async () => "B");
    expect(a).toBe("A");
    expect(b).toBe("B");
  });

  it("does not cache a rejected promise, so the next call retries", async () => {
    const store = new Map<string, Promise<number>>();
    let attempt = 0;
    const compute = vi.fn(async () => {
      attempt += 1;
      if (attempt === 1) throw new Error("transient failure");
      return 7;
    });

    await expect(memoizeAsync(store, "k", compute)).rejects.toThrow("transient failure");
    const result = await memoizeAsync(store, "k", compute);

    expect(result).toBe(7);
    expect(compute).toHaveBeenCalledTimes(2);
  });

  it("returns the in-flight promise for concurrent calls before it settles", async () => {
    const store = new Map<string, Promise<number>>();
    let resolveCompute: (v: number) => void = () => {};
    const compute = vi.fn(
      () =>
        new Promise<number>((resolve) => {
          resolveCompute = resolve;
        }),
    );

    const first = memoizeAsync(store, "k", compute);
    const second = memoizeAsync(store, "k", compute);
    resolveCompute(99);

    expect(await first).toBe(99);
    expect(await second).toBe(99);
    expect(compute).toHaveBeenCalledTimes(1);
  });
});
