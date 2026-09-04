/**
 * Minimal ambient types for Bun's built-in test runner. The shim exists
 * because `bun test` needs no dependency but `tsc` type-checks everything
 * under `src`, so `import { test } from "bun:test"` would not resolve. It
 * declares only the matchers the tests use. `@types/bun` would pull
 * `bun-types` into the frontend build, where it overrides DOM and Node globals
 * for every file.
 */
declare module "bun:test" {
  interface Matchers {
    toBe(expected: unknown): void;
    toEqual(expected: unknown): void;
    toBeNull(): void;
    toBeUndefined(): void;
    toContain(expected: unknown): void;
    toBeGreaterThan(expected: number): void;
    toBeGreaterThanOrEqual(expected: number): void;
    toBeLessThan(expected: number): void;
    toBeLessThanOrEqual(expected: number): void;
    toBeCloseTo(expected: number, numDigits?: number): void;
    toThrow(expected?: unknown): void;
    readonly not: Matchers;
  }

  export function describe(label: string, body: () => void): void;
  export function test(label: string, body: () => void | Promise<void>): void;
  export function beforeAll(body: () => void | Promise<void>): void;
  export function afterAll(body: () => void | Promise<void>): void;
  export function expect(value: unknown): Matchers;
}
