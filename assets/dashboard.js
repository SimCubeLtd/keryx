(function () {
  "use strict";

  var themeKey = "keryx-theme";
  var themeSelect = document.getElementById("theme-select");
  var storedTheme = "system";
  try {
    storedTheme = localStorage.getItem(themeKey) || "system";
  } catch (_) {}
  if (!["system", "light", "dark"].includes(storedTheme)) storedTheme = "system";
  document.documentElement.dataset.theme = storedTheme;
  themeSelect.value = storedTheme;
  themeSelect.addEventListener("change", function () {
    document.documentElement.dataset.theme = themeSelect.value;
    try { localStorage.setItem(themeKey, themeSelect.value); } catch (_) {}
  });

  var VIEWS = ["active", "snoozed", "disabled"];
  var rows = Array.from(document.querySelectorAll(".draft-row"));
  var tbody = document.getElementById("draft-rows");
  var search = document.getElementById("draft-search");
  var tabs = Array.from(document.querySelectorAll("[data-view]"));
  var repoFilter = document.getElementById("repo-filter");
  var sort = document.getElementById("draft-sort");
  var resultCount = document.getElementById("result-count");
  var emptyResults = document.getElementById("empty-results");
  var timeColumn = document.getElementById("time-column");
  var managementEnabled = document.body.dataset.managementEnabled === "true";
  var selectedId = null;
  var view = "active";

  function relativeTime(value) {
    var timestamp = Date.parse(value);
    if (!Number.isFinite(timestamp)) return value || "Not recorded";
    var seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1000));
    if (seconds < 60) return "Just now";
    var minutes = Math.floor(seconds / 60);
    if (minutes < 60) return minutes + "m ago";
    var hours = Math.floor(minutes / 60);
    if (hours < 24) return hours + "h ago";
    var days = Math.floor(hours / 24);
    if (days < 14) return days + "d ago";
    return new Date(timestamp).toLocaleDateString();
  }

  function relativeFuture(value) {
    var timestamp = Date.parse(value);
    if (!Number.isFinite(timestamp)) return "";
    var seconds = Math.max(0, Math.floor((timestamp - Date.now()) / 1000));
    if (seconds < 60) return "in under a minute";
    var minutes = Math.floor(seconds / 60);
    if (minutes < 60) return "in " + minutes + (minutes === 1 ? " minute" : " minutes");
    var hours = Math.floor(minutes / 60);
    if (hours < 48) return "in " + hours + (hours === 1 ? " hour" : " hours");
    var days = Math.floor(hours / 24);
    return "in " + days + " days";
  }

  function wakeLabel(value) {
    var timestamp = Date.parse(value);
    if (!Number.isFinite(timestamp)) return value || "";
    return new Date(timestamp).toLocaleString(undefined, {
      weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit"
    });
  }

  function fileSize(bytes) {
    if (!Number.isFinite(bytes)) return "";
    if (bytes < 1024) return bytes + " B";
    if (bytes < 1024 * 1024) return Math.round(bytes / 1024) + " KB";
    return (bytes / (1024 * 1024)).toFixed(1) + " MB";
  }

  function setText(id, value) {
    var element = document.getElementById(id);
    if (element) element.textContent = value || "Not recorded";
  }

  // Availability is derived the same way the server derives it: disabled
  // wins, then a future wake time means snoozed, otherwise active.
  function deriveAvailability(row) {
    if (row.dataset.disabled === "true") return "disabled";
    var until = Date.parse(row.dataset.snoozedUntil || "");
    return Number.isFinite(until) && until > Date.now() ? "snoozed" : "active";
  }

  function syncAvailability(row) {
    var derived = deriveAvailability(row);
    var changed = row.dataset.availability !== derived;
    row.dataset.availability = derived;
    return changed;
  }

  function renderRowTime(row) {
    var time = row.querySelector(".updated");
    var detail = row.querySelector(".version");
    if (row.dataset.availability === "snoozed") {
      time.textContent = "Wakes " + relativeFuture(row.dataset.snoozedUntil);
      time.title = row.dataset.snoozedUntil;
      time.classList.add("wake");
      detail.textContent = wakeLabel(row.dataset.snoozedUntil);
    } else {
      time.textContent = relativeTime(row.dataset.updated);
      time.title = row.dataset.updated;
      time.classList.remove("wake");
      var count = Number(row.dataset.versionCount);
      detail.textContent = "v" + row.dataset.latestVersion + " · " + count + (count === 1 ? " version" : " versions");
    }
  }

  function updateCounts() {
    var counts = { active: 0, snoozed: 0, disabled: 0 };
    rows.forEach(function (row) { counts[row.dataset.availability] += 1; });
    VIEWS.forEach(function (key) {
      var badge = document.querySelector('[data-count="' + key + '"]');
      if (badge) badge.textContent = counts[key];
    });
    return counts;
  }

  function populateSummary(row, keepHistory) {
    if (!row) return;
    var changedSelection = selectedId !== row.dataset.draftId;
    selectedId = row.dataset.draftId;
    rows.forEach(function (candidate) {
      var selected = candidate === row;
      candidate.classList.toggle("selected", selected);
      candidate.setAttribute("aria-selected", String(selected));
    });

    var state = row.dataset.availability;
    var status = document.getElementById("detail-status");
    status.textContent = state.charAt(0).toUpperCase() + state.slice(1);
    status.className = "status " + state;
    setText("detail-title", row.dataset.title);
    setText("detail-description", row.dataset.description || "No description supplied.");
    setText("detail-origin", row.dataset.repoHost);
    setText("detail-repository", row.dataset.repository);
    setText("detail-branch", row.dataset.branch);
    setText("detail-id", row.dataset.draftId);
    setText("detail-updated", relativeTime(row.dataset.updated));
    setText("detail-version", "Version " + row.dataset.latestVersion + " of " + row.dataset.versionCount);
    setText("detail-commit", row.dataset.commitSubject || row.dataset.commitSha);

    var availability = document.getElementById("detail-availability");
    availability.hidden = state === "active";
    availability.className = "detail-availability " + state;
    if (state === "snoozed") {
      availability.textContent = "Snoozed until " + wakeLabel(row.dataset.snoozedUntil) + " (" + relativeFuture(row.dataset.snoozedUntil) + "). Public links keep working while it sleeps.";
    } else if (state === "disabled") {
      availability.textContent = "Disabled. Public, raw, versioned, and PDF links return 404 until the draft is enabled.";
    }

    var open = document.getElementById("detail-open");
    var pdf = document.getElementById("detail-pdf");
    var unavailable = state === "disabled";
    open.href = row.dataset.publicUrl;
    open.setAttribute("aria-disabled", String(unavailable));
    if (pdf) {
      pdf.href = "/api/drafts/" + encodeURIComponent(row.dataset.draftId) + "/pdf";
      pdf.setAttribute("aria-disabled", String(unavailable));
    }
    var visibleActions = { active: ["snooze", "disable"], snoozed: ["unsnooze", "disable"], disabled: ["enable"] }[state];
    document.querySelectorAll("[data-availability-action]").forEach(function (button) {
      button.hidden = !visibleActions.includes(button.dataset.availabilityAction);
    });

    if (keepHistory && !changedSelection) return;
    if (managementEnabled) {
      loadVersions(row.dataset.draftId);
    } else {
      document.getElementById("history-state").textContent = "Use authenticated CLI";
      document.getElementById("version-history").replaceChildren();
    }
  }

  function renderVersions(versions, draftId, expanded) {
    var history = document.getElementById("version-history");
    history.replaceChildren();
    var visible = expanded ? versions : versions.slice(0, 8);
    visible.forEach(function (version) {
      var item = document.createElement("a");
      item.className = "history-item";
      item.href = "/d/" + encodeURIComponent(draftId) + "/v/" + encodeURIComponent(String(version.versionNumber));
      item.target = "_blank";
      item.rel = "noopener noreferrer";
      item.setAttribute("aria-label", "Open version " + version.versionNumber + " in a new tab");
      var badge = document.createElement("span");
      badge.className = "vbadge";
      badge.textContent = "v" + version.versionNumber;
      var content = document.createElement("div");
      var commit = document.createElement("div");
      commit.className = "commit";
      commit.textContent = version.gitCommitSubject || version.originalFilename || "Published version";
      var metadata = document.createElement("div");
      metadata.className = "commit-meta";
      var repository = version.repoOrg && version.repoName ? version.repoOrg + "/" + version.repoName : "provenance not recorded";
      var branch = version.gitBranch || "branch not recorded";
      var sha = version.gitCommitSha ? version.gitCommitSha.slice(0, 8) : "no commit";
      metadata.textContent = repository + " · " + branch + " · " + sha + " · " + fileSize(version.fileSize);
      content.append(commit, metadata);
      var time = document.createElement("time");
      time.className = "history-time";
      time.textContent = relativeTime(version.createdAt);
      var open = document.createElement("span");
      open.className = "history-open";
      open.textContent = "↗";
      item.append(badge, content, time, open);
      history.append(item);
    });

    if (!expanded && visible.length < versions.length) {
      var remaining = versions.length - visible.length;
      var footer = document.createElement("div");
      footer.className = "history-more";
      var count = document.createElement("div");
      count.className = "history-count";
      var countLabel = document.createElement("strong");
      countLabel.textContent = "Showing " + visible.length + " of " + versions.length;
      var order = document.createElement("span");
      order.textContent = "Newest versions first";
      count.append(countLabel, order);
      var button = document.createElement("button");
      button.className = "history-more-button";
      button.type = "button";
      button.textContent = "Load " + remaining + " older " + (remaining === 1 ? "version" : "versions");
      button.addEventListener("click", function () { renderVersions(versions, draftId, true); });
      footer.append(count, button);
      history.append(footer);
    }
  }

  function loadVersions(draftId) {
    var state = document.getElementById("history-state");
    state.textContent = "Loading…";
    fetch("/api/drafts/" + encodeURIComponent(draftId), { headers: { Accept: "application/json" } })
      .then(function (response) {
        if (!response.ok) {
          var error = new Error(response.status === 401 ? "API key required" : "Version history unavailable");
          error.status = response.status;
          throw error;
        }
        return response.json();
      })
      .then(function (body) {
        if (selectedId !== draftId) return;
        var versions = body.draft.versions || [];
        state.textContent = versions.length + (versions.length === 1 ? " version" : " versions");
        renderVersions(versions, draftId, false);
      })
      .catch(function (error) {
        if (selectedId !== draftId) return;
        state.textContent = error.message;
        document.getElementById("version-history").replaceChildren();
      });
  }

  function matches(row, query) {
    if (row.dataset.availability !== view) return false;
    if (query && !row.dataset.search.includes(query)) return false;
    if (repoFilter && repoFilter.value && row.dataset.repository !== repoFilter.value) return false;
    return true;
  }

  var EMPTY_HINTS = {
    active: "Publish one with keryx upload ./plan.html",
    snoozed: "Snooze a draft to park it until a wake time.",
    disabled: "Disable a draft to stop serving it."
  };

  function applyFilters() {
    var query = search.value.trim().toLocaleLowerCase();
    var counts = updateCounts();
    var visible = [];
    rows.forEach(function (row) {
      var show = matches(row, query);
      row.hidden = !show;
      if (show) visible.push(row);
    });
    resultCount.textContent = (visible.length === counts[view] ? "" : visible.length + " of ") + counts[view] + " " + view;
    emptyResults.hidden = visible.length !== 0;
    if (counts[view] === 0) {
      setText("empty-title", "No " + view + " drafts");
      setText("empty-hint", EMPTY_HINTS[view]);
    } else {
      setText("empty-title", "No matching drafts");
      setText("empty-hint", "Adjust the search or repository filter.");
    }
    var detail = document.getElementById("draft-detail");
    if (!visible.some(function (row) { return row.dataset.draftId === selectedId; })) {
      if (visible[0]) {
        detail.classList.remove("empty");
        populateSummary(visible[0]);
      } else {
        detail.classList.add("empty");
      }
    } else {
      detail.classList.remove("empty");
    }
  }

  function applySort() {
    var direction = sort.value;
    rows.sort(function (left, right) {
      if (direction === "title") return left.dataset.title.localeCompare(right.dataset.title);
      if (direction === "versions") return Number(right.dataset.versionCount) - Number(left.dataset.versionCount);
      var delta = Date.parse(right.dataset.updated) - Date.parse(left.dataset.updated);
      return direction === "oldest" ? -delta : delta;
    });
    rows.forEach(function (row) { tbody.append(row); });
    applyFilters();
  }

  function setView(next) {
    if (!VIEWS.includes(next)) next = "active";
    view = next;
    tabs.forEach(function (tab) { tab.setAttribute("aria-selected", String(tab.dataset.view === view)); });
    timeColumn.textContent = view === "snoozed" ? "Wakes" : "Updated";
    search.placeholder = "Search " + view + " drafts…";
    applyFilters();
  }

  function syncUrl() {
    var params = new URLSearchParams();
    if (selectedId) params.set("draft", selectedId);
    params.set("view", view);
    history.replaceState(null, "", "/?" + params.toString());
  }

  function findRow(draftId) {
    return rows.find(function (candidate) { return candidate.dataset.draftId === draftId; });
  }

  tabs.forEach(function (tab) {
    tab.addEventListener("click", function () {
      setView(tab.dataset.view);
      syncUrl();
    });
  });
  search.addEventListener("input", applyFilters);
  if (repoFilter) repoFilter.addEventListener("change", applyFilters);
  sort.addEventListener("change", applySort);
  document.addEventListener("keydown", function (event) {
    if (event.key === "/" && document.activeElement !== search) {
      event.preventDefault();
      search.focus();
    }
  });

  rows.forEach(function (row) {
    syncAvailability(row);
    renderRowTime(row);
    row.addEventListener("click", function (event) {
      if (event.target.closest("a, button, details, summary")) return;
      populateSummary(row);
      syncUrl();
    });
    row.addEventListener("keydown", function (event) {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        populateSummary(row);
        syncUrl();
      }
    });
  });

  document.addEventListener("click", function (event) {
    document.querySelectorAll(".row-menu[open]").forEach(function (menu) {
      if (!menu.contains(event.target)) menu.removeAttribute("open");
    });
  });

  // --- availability -----------------------------------------------------

  var wakeTimer = null;

  // Snoozes expire on the client the same way they do on the server: by the
  // clock, without a request. Rows that pass their wake time move to Active.
  function scheduleWake() {
    if (wakeTimer) clearTimeout(wakeTimer);
    wakeTimer = null;
    var next = Infinity;
    rows.forEach(function (row) {
      if (row.dataset.availability !== "snoozed") return;
      next = Math.min(next, Date.parse(row.dataset.snoozedUntil));
    });
    if (!Number.isFinite(next)) return;
    var delay = Math.min(Math.max(next - Date.now(), 0) + 500, 24 * 60 * 60 * 1000);
    wakeTimer = setTimeout(function () {
      var woke = rows.filter(function (row) { return syncAvailability(row); });
      woke.forEach(function (row) {
        renderRowTime(row);
        if (row.dataset.draftId === selectedId) populateSummary(row, true);
      });
      if (woke.length === 1) showToast("“" + woke[0].dataset.title + "” woke and is active again.", false);
      else if (woke.length > 1) showToast(woke.length + " drafts woke and are active again.", false);
      applyFilters();
      scheduleWake();
    }, delay);
  }

  function applyDraft(row, draft) {
    row.dataset.disabled = String(Boolean(draft.disabled));
    row.dataset.snoozedUntil = draft.snoozedUntil || "";
    row.dataset.updated = draft.updatedAt || row.dataset.updated;
    syncAvailability(row);
    renderRowTime(row);
    if (row.dataset.draftId === selectedId) populateSummary(row, true);
    applyFilters();
    syncUrl();
    scheduleWake();
  }

  function setAvailability(row, update, describe) {
    var buttons = Array.from(document.querySelectorAll("[data-availability-action]"));
    buttons.forEach(function (button) { button.disabled = true; });
    return fetch("/api/drafts/" + encodeURIComponent(row.dataset.draftId) + "/availability", {
      method: "PUT",
      headers: { Accept: "application/json", "Content-Type": "application/json" },
      body: JSON.stringify(update)
    })
      .then(function (response) {
        return response.json().catch(function () { return {}; }).then(function (body) {
          if (!response.ok) {
            throw new Error(response.status === 401 ? "API key required. Use the CLI for this action." : (body.error || "The draft could not be updated."));
          }
          return body;
        });
      })
      .then(function (body) {
        applyDraft(row, body.draft);
        showToast(describe(body.draft), false);
      })
      .catch(function (error) { showToast(error.message, true); })
      .finally(function () { buttons.forEach(function (button) { button.disabled = false; }); });
  }

  var snoozeDialog = document.getElementById("snooze-dialog");
  var snoozeInput = document.getElementById("snooze-until");
  var snoozePreview = document.getElementById("snooze-preview");
  var presets = Array.from(document.querySelectorAll("[data-preset]"));

  function toLocalInput(date) {
    var pad = function (n) { return String(n).padStart(2, "0"); };
    return date.getFullYear() + "-" + pad(date.getMonth() + 1) + "-" + pad(date.getDate()) + "T" + pad(date.getHours()) + ":" + pad(date.getMinutes());
  }

  function presetDate(preset) {
    var date = new Date();
    if (preset === "1h") date.setHours(date.getHours() + 1);
    else if (preset === "4h") date.setHours(date.getHours() + 4);
    else if (preset === "tomorrow") { date.setDate(date.getDate() + 1); date.setHours(9, 0, 0, 0); }
    else if (preset === "monday") {
      var ahead = (8 - date.getDay()) % 7 || 7;
      date.setDate(date.getDate() + ahead);
      date.setHours(9, 0, 0, 0);
    }
    date.setSeconds(0, 0);
    return date;
  }

  function updateSnoozePreview() {
    var timestamp = Date.parse(snoozeInput.value);
    var confirm = document.getElementById("snooze-confirm");
    if (!Number.isFinite(timestamp) || timestamp <= Date.now()) {
      snoozePreview.textContent = "Pick a wake time in the future.";
      confirm.disabled = true;
      return;
    }
    snoozePreview.textContent = "Wakes " + wakeLabel(new Date(timestamp).toISOString()) + " (" + relativeFuture(new Date(timestamp).toISOString()) + ")";
    confirm.disabled = false;
  }

  function openSnoozeDialog(row) {
    setText("snooze-draft-title", row.dataset.title);
    setText("snooze-draft-id", row.dataset.draftId);
    presets.forEach(function (preset) { preset.setAttribute("aria-pressed", String(preset.dataset.preset === "tomorrow")); });
    snoozeInput.min = toLocalInput(new Date());
    snoozeInput.value = toLocalInput(presetDate("tomorrow"));
    updateSnoozePreview();
    snoozeDialog.showModal();
  }

  if (managementEnabled) {
    presets.forEach(function (preset) {
      preset.addEventListener("click", function () {
        presets.forEach(function (candidate) { candidate.setAttribute("aria-pressed", String(candidate === preset)); });
        snoozeInput.value = toLocalInput(presetDate(preset.dataset.preset));
        updateSnoozePreview();
      });
    });
    snoozeInput.addEventListener("input", function () {
      presets.forEach(function (candidate) { candidate.setAttribute("aria-pressed", "false"); });
      updateSnoozePreview();
    });
    document.getElementById("snooze-cancel").addEventListener("click", function () { snoozeDialog.close(); });
    document.getElementById("snooze-confirm").addEventListener("click", function () {
      var row = findRow(selectedId);
      var timestamp = Date.parse(snoozeInput.value);
      if (!row || !Number.isFinite(timestamp)) return;
      snoozeDialog.close();
      setAvailability(row, { state: "snoozed", until: new Date(timestamp).toISOString() }, function (draft) {
        return "Snoozed until " + wakeLabel(draft.snoozedUntil) + ". Public links keep working.";
      });
    });

    document.querySelectorAll("[data-availability-action]").forEach(function (button) {
      button.addEventListener("click", function () {
        var row = findRow(selectedId);
        if (!row) return;
        var action = button.dataset.availabilityAction;
        if (action === "snooze") return openSnoozeDialog(row);
        if (action === "disable") {
          return setAvailability(row, { state: "disabled" }, function () { return "Draft disabled. Public links now return 404."; });
        }
        setAvailability(row, { state: "active" }, function () {
          return action === "unsnooze" ? "Draft is active again." : "Draft enabled. Public links work again.";
        });
      });
    });
  }

  // --- prune ------------------------------------------------------------

  var dialog = document.getElementById("prune-dialog");
  var confirmAction = document.getElementById("confirm-action");
  var pending = null;

  function openPruneDialog(button) {
    pending = { id: button.dataset.draftId, title: button.dataset.title, action: button.dataset.action };
    var force = pending.action === "purge";
    setText("dialog-title", force ? "Permanently delete this draft?" : "Prune this draft?");
    setText("dialog-description", force
      ? "Force prune removes the draft metadata, every stored version, and each HTML file."
      : "Prune stops serving the draft and removes it from the dashboard. Stored versions remain for housekeeping.");
    setText("dialog-draft-title", pending.title);
    setText("dialog-draft-id", pending.id);
    setText("dialog-warning", force
      ? "This cannot be undone. Public and versioned URLs will stop working."
      : "This is a soft delete. Run force prune or keryx purge later to remove the stored files.");
    confirmAction.textContent = force ? "Force prune" : "Prune draft";
    confirmAction.classList.toggle("danger", force);
    dialog.showModal();
  }

  if (managementEnabled) {
    document.querySelectorAll("[data-action]").forEach(function (button) {
      button.addEventListener("click", function () { openPruneDialog(button); });
    });
    document.getElementById("cancel-action").addEventListener("click", function () { dialog.close(); });
    confirmAction.addEventListener("click", function () {
      if (!pending) return;
      confirmAction.disabled = true;
      var suffix = pending.action === "purge" ? "?purge=true" : "";
      fetch("/api/drafts/" + encodeURIComponent(pending.id) + suffix, { method: "DELETE", headers: { Accept: "application/json" } })
        .then(function (response) {
          if (!response.ok) throw new Error(response.status === 401 ? "API key required. Use the CLI for this action." : "The draft could not be pruned.");
          return response.json();
        })
        .then(function () {
          var row = findRow(pending.id);
          if (row) {
            rows = rows.filter(function (candidate) { return candidate !== row; });
            row.remove();
          }
          dialog.close();
          showToast(pending.action === "purge" ? "Draft permanently deleted." : "Draft pruned.", false);
          pending = null;
          applyFilters();
          scheduleWake();
        })
        .catch(function (error) { showToast(error.message, true); })
        .finally(function () { confirmAction.disabled = false; });
    });
  }

  function showToast(message, error) {
    var toast = document.getElementById("toast");
    toast.textContent = message;
    toast.classList.toggle("error", error);
    toast.hidden = false;
    window.setTimeout(function () { toast.hidden = true; }, 4200);
  }

  // --- deep links -------------------------------------------------------
  // /?draft=<id>&view=<availability> selects a tab and a draft. A draft that
  // lives in another tab wins over the view parameter.

  var params = new URLSearchParams(location.search);
  var linkedRow = params.get("draft") ? findRow(params.get("draft")) : null;
  var linkedView = params.get("view") || "active";
  if (linkedRow) linkedView = linkedRow.dataset.availability;
  applySort();
  setView(linkedView);
  if (linkedRow && !linkedRow.hidden) {
    populateSummary(linkedRow);
    linkedRow.scrollIntoView({ block: "nearest" });
  }
  scheduleWake();
})();
