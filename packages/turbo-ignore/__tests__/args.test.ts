import { spyConsole, spyExit } from "@turbo/test-utils";
import { parseArgs, help } from "../src/args";
import pkg from "../package.json";

describe("parseArgs()", () => {
  const mockConsole = spyConsole();
  const mockExit = spyExit();

  it("does not throw with no args", () => {
    const result = parseArgs({ argv: [] });
    expect(result.workspace).toBe(undefined);
    expect(result.fallback).toBe(undefined);
    expect(result.task).toBe(undefined);
  });

  it("outputs help text (--help)", () => {
    parseArgs({ argv: ["--help"] });
    expect(mockExit.exit).toHaveBeenCalledWith(0);
    expect(mockConsole.log).toHaveBeenCalledWith(help);
  });

  it("outputs help text (-h)", () => {
    parseArgs({ argv: ["-h"] });
    expect(mockExit.exit).toHaveBeenCalledWith(0);
    expect(mockConsole.log).toHaveBeenCalledWith(help);
  });

  it("outputs version text (--version)", () => {
    parseArgs({ argv: ["--version"] });
    expect(mockExit.exit).toHaveBeenCalledWith(0);
    expect(mockConsole.log).toHaveBeenCalledWith(pkg.version);
  });

  it("outputs version text (-v)", () => {
    parseArgs({ argv: ["-v"] });
    expect(mockExit.exit).toHaveBeenCalledWith(0);
    expect(mockConsole.log).toHaveBeenCalledWith(pkg.version);
  });

  it("correctly finds workspace", () => {
    const result = parseArgs({ argv: ["this-workspace"] });
    expect(result.workspace).toBe("this-workspace");
    expect(result.fallback).toBe(undefined);
    expect(result.task).toBe(undefined);
    expect(mockExit.exit).toHaveBeenCalledTimes(0);
  });

  it("correctly finds fallback", () => {
    const result = parseArgs({ argv: ["--fallback=HEAD^"] });
    expect(result.workspace).toBe(undefined);
    expect(result.fallback).toBe("HEAD^");
    expect(result.task).toBe(undefined);
    expect(mockExit.exit).toHaveBeenCalledTimes(0);
  });

  it("correctly finds task", () => {
    const result = parseArgs({ argv: ["--task=some-workspace#build"] });
    expect(result.workspace).toBe(undefined);
    expect(result.fallback).toBe(undefined);
    expect(result.task).toBe("some-workspace#build");
    expect(mockExit.exit).toHaveBeenCalledTimes(0);
  });

  it("uses default fallback if incorrectly specified", () => {
    const result = parseArgs({ argv: ["--fallback"] });
    expect(result.workspace).toBe(undefined);
    expect(result.fallback).toBe(undefined);
    expect(result.task).toBe(undefined);
    expect(mockExit.exit).toHaveBeenCalledTimes(0);
  });

  it("uses default fallback if empty string", () => {
    const result = parseArgs({ argv: ["--fallback="] });
    expect(result.workspace).toBe(undefined);
    expect(result.fallback).toBe(undefined);
    expect(result.task).toBe(undefined);
    expect(mockExit.exit).toHaveBeenCalledTimes(0);
  });

  it("uses default task if incorrectly specified", () => {
    const result = parseArgs({ argv: ["--task"] });
    expect(result.workspace).toBe(undefined);
    expect(result.fallback).toBe(undefined);
    expect(result.task).toBe(undefined);
    expect(mockExit.exit).toHaveBeenCalledTimes(0);
  });

  it("uses default task if empty string", () => {
    const result = parseArgs({ argv: ["--task="] });
    expect(result.workspace).toBe(undefined);
    expect(result.fallback).toBe(undefined);
    expect(result.task).toBe(undefined);
    expect(mockExit.exit).toHaveBeenCalledTimes(0);
  });

  it("correctly finds fallback and workspace", () => {
    const result = parseArgs({
      argv: [
        "this-workspace",
        "--fallback=HEAD~10",
        "--task=some-workspace#build",
      ],
    });
    expect(result.workspace).toBe("this-workspace");
    expect(result.fallback).toBe("HEAD~10");
    expect(result.task).toBe("some-workspace#build");
    expect(mockExit.exit).toHaveBeenCalledTimes(0);
  });
});
