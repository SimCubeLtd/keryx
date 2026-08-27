/* Keryx service worker. It exists so the browser can receive Web Push and
   route notification clicks while the dashboard is closed. It never
   intercepts requests and keeps no offline copy: drafts are served live. */

self.addEventListener("install", function () {
  self.skipWaiting();
});

self.addEventListener("activate", function (event) {
  event.waitUntil(self.clients.claim());
});

// Only a same-origin absolute path is ever navigated to. Anything else in a
// payload falls back to the dashboard root.
function sameOriginPath(value) {
  if (typeof value !== "string" || value.charAt(0) !== "/" || value.charAt(1) === "/") return "/";
  return value;
}

self.addEventListener("push", function (event) {
  var payload = {};
  try {
    payload = event.data ? event.data.json() : {};
  } catch (_) {
    payload = { body: event.data ? event.data.text() : "" };
  }
  var title = typeof payload.title === "string" && payload.title ? payload.title : "Keryx";
  event.waitUntil(self.registration.showNotification(title, {
    body: typeof payload.body === "string" ? payload.body : "",
    tag: typeof payload.tag === "string" ? payload.tag : undefined,
    icon: "/pwa-icon-192.png",
    badge: "/pwa-icon-192.png",
    data: { target: sameOriginPath(payload.target) }
  }));
});

self.addEventListener("notificationclick", function (event) {
  event.notification.close();
  var target = sameOriginPath(event.notification.data && event.notification.data.target);
  var url = new URL(target, self.location.origin).href;
  event.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then(function (windows) {
      var existing = windows.find(function (client) {
        return new URL(client.url).origin === self.location.origin;
      });
      if (!existing) return self.clients.openWindow(url);
      return existing.focus().then(function (focused) {
        var client = focused || existing;
        return client.navigate ? client.navigate(url) : client;
      });
    })
  );
});
