it("should load chunk a", async () => {
  await expect(import("./a")).resolves.toHaveProperty("default", "a");
});

it("should load chunk b", async () => {
  await expect(import("./b")).resolves.toHaveProperty("default", "b");
});
