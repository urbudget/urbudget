// The Android test's two steps, on the home page and /android/: join the testers group on Google
// Groups (the closed test's tester list in Play Console), then become a tester on Google Play.
// Once the group is opened, its step shows as done and Google Play becomes the button to press.
// Without this script both links still work.

for (const steps of document.querySelectorAll(".join-steps")) {
  const group = steps.querySelector('[data-join="group"]');
  const play = steps.querySelector('[data-join="play"]');
  group?.addEventListener("click", () => {
    const step = group.closest(".join-step");
    step?.classList.add("is-done");
    const num = step?.querySelector(".join-num");
    if (num) num.textContent = "✓";
    group.classList.remove("btn-primary");
    play?.classList.add("btn-primary");
  });
}
