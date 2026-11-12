// The Android test sign-up, on the home page and /android/: the email of a Google account to add
// to Google Play's closed test. Without this script the form does nothing, and the page still reads.
import { api } from "./api.js";

function androidSignup() {
  const form = document.getElementById("android-form");
  if (!form) return;
  const input = form.querySelector("#android-email");
  const button = form.querySelector("#android-submit");
  const status = form.querySelector("#android-status");
  const say = (message, kind) => {
    status.textContent = message;
    status.classList.toggle("is-error", kind === "error");
    status.classList.toggle("is-success", kind === "success");
  };
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const email = input.value.trim();
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
      say("Enter the email address of your Google account.", "error");
      input.setAttribute("aria-invalid", "true");
      input.focus();
      return;
    }
    input.removeAttribute("aria-invalid");
    button.disabled = true;
    say("");
    try {
      await api("/android-testers", { method: "POST", body: { email } });
      say(`You're on the list. When Google has approved the test, we'll send the link to ${email}.`, "success");
      input.value = "";
    } catch (error) {
      say(error.message, "error");
    } finally {
      button.disabled = false;
    }
  });
}

androidSignup();
