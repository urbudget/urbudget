// The Android test's joining card, on the home page and /android/: open the testers group on Google
// Groups (the closed test's tester list in Play Console), tap Join group there, then become a tester
// on Google Play. Once the group is opened, step 1 shows as done, step 2 becomes the current one and
// Google Play becomes the primary button. Without this script both links still work.

for (const card of document.querySelectorAll(".join")) {
  const group = card.querySelector('[data-join="group"]');
  const play = card.querySelector('[data-join="play"]');
  group?.addEventListener("click", () => {
    const step = group.closest(".join-step");
    if (!step || step.classList.contains("is-done")) return;
    step.classList.replace("is-current", "is-done");
    step.classList.add("is-done");
    const num = step.querySelector(".join-num");
    if (num) num.textContent = "✓";
    const title = step.querySelector(".join-title");
    if (title) title.insertAdjacentHTML("beforeend", '<span class="visually-hidden"> (done)</span>');
    step.nextElementSibling?.classList.add("is-current");
    group.classList.remove("btn-primary");
    play?.classList.add("btn-primary");
  });
}
