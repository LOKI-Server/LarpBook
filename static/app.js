// Small touches on top of pages that work without JavaScript.
"use strict";

// Times in the reader's own zone: "5m", "3h", then a date.
const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto", style: "narrow" });
const sameYear = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const otherYear = new Intl.DateTimeFormat(undefined, { year: "numeric", month: "short", day: "numeric" });
const full = new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "short" });
function when(date) {
  const seconds = (Date.now() - date) / 1000;
  if (seconds < 45) return "just now";
  if (seconds < 3600) return relative.format(-Math.round(seconds / 60), "minute");
  if (seconds < 86400) return relative.format(-Math.round(seconds / 3600), "hour");
  if (seconds < 7 * 86400) return relative.format(-Math.round(seconds / 86400), "day");
  return (date.getFullYear() === new Date().getFullYear() ? sameYear : otherYear).format(date);
}
function times() {
  for (const time of document.querySelectorAll("time[datetime]")) {
    const date = new Date(time.dateTime);
    if (Number.isNaN(date.getTime())) continue;
    time.textContent = when(date);
    time.title = full.format(date);
  }
}

// A character count that appears near the limit, and Ctrl/⌘+Enter to send.
function composers() {
  for (const box of document.querySelectorAll("textarea[maxlength]")) {
    const count = box.form.querySelector(".count");
    const update = () => {
      const left = box.maxLength - [...box.value].length;
      count.textContent = left <= 200 ? `${left} left` : "";
      count.classList.toggle("over", left < 0);
    };
    box.addEventListener("input", update);
    box.addEventListener("keydown", event => {
      if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) box.form.requestSubmit();
    });
    update();
  }
}

document.addEventListener("submit", event => {
  const message = event.target.dataset.confirm;
  if (message && !confirm(message)) event.preventDefault();
});

// A picture that fails to load leaves the initial underneath showing.
document.addEventListener("error", event => {
  if (event.target.tagName === "IMG") event.target.remove();
}, true);

document.addEventListener("DOMContentLoaded", () => {
  times();
  composers();
  setInterval(times, 60000);
});
