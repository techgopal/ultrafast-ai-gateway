/* global window, document */
// Applies the saved theme before the first paint. Loaded from <head> as a file,
// because the Content Security Policy allows no inline script.
(function () {
  var choice = null;
  try {
    choice = window.localStorage.getItem("uf-theme");
  } catch {
    // Storage is blocked: follow the device.
  }
  var dark =
    choice === "dark" ||
    (choice !== "light" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  var root = document.documentElement;
  root.classList.toggle("dark", dark);
  root.style.colorScheme = dark ? "dark" : "light";
})();
