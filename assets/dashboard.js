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

  var rows = Array.from(document.querySelectorAll(".draft-row"));
  var tbody = document.getElementById("draft-rows");
  var search = document.getElementById("draft-search");
  var filters = Array.from(document.querySelectorAll("[data-filter]"));
  var repoFilter = document.getElementById("repo-filter");
  var sort = document.getElementById("draft-sort");
  var resultCount = document.getElementById("result-count");
  var emptyResults = document.getElementById("empty-results");
  var managementEnabled = document.body.dataset.managementEnabled === "true";
  var selectedId = null;

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

  function populateSummary(row) {
    if (!row) return;
    selectedId = row.dataset.draftId;
    rows.forEach(function (candidate) {
      var selected = candidate === row;
      candidate.classList.toggle("selected", selected);
      candidate.setAttribute("aria-selected", String(selected));
    });

    var disabled = row.dataset.disabled === "true";
    var status = document.getElementById("detail-status");
    status.textContent = disabled ? "Disabled" : "Active";
    status.classList.toggle("disabled", disabled);
    setText("detail-title", row.dataset.title);
    setText("detail-description", row.dataset.description || "No description supplied.");
    setText("detail-origin", row.dataset.repoHost);
    setText("detail-repository", row.dataset.repository);
    setText("detail-branch", row.dataset.branch);
    setText("detail-id", row.dataset.draftId);
    setText("detail-updated", relativeTime(row.dataset.updated));
    setText("detail-version", "Version " + row.dataset.latestVersion + " of " + row.dataset.versionCount);
    setText("detail-commit", row.dataset.commitSubject || row.dataset.commitSha);

    var open = document.getElementById("detail-open");
    var pdf = document.getElementById("detail-pdf");
    open.href = row.dataset.publicUrl;
    var unavailable = disabled;
    open.setAttribute("aria-disabled", String(unavailable));
    if (pdf) {
      pdf.href = "/api/drafts/" + encodeURIComponent(row.dataset.draftId) + "/pdf";
      pdf.setAttribute("aria-disabled", String(unavailable));
    }
    if (managementEnabled) {
      loadVersions(row.dataset.draftId);
    } else {
      document.getElementById("history-state").textContent = "Use authenticated CLI";
      document.getElementById("version-history").replaceChildren();
    }
  }

  function renderVersions(versions) {
    var history = document.getElementById("version-history");
    history.replaceChildren();
    versions.slice(0, 8).forEach(function (version) {
      var item = document.createElement("div");
      item.className = "history-item";
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
      item.append(badge, content, time);
      history.append(item);
    });
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
        renderVersions(versions);
      })
      .catch(function (error) {
        if (selectedId !== draftId) return;
        state.textContent = error.message;
        document.getElementById("version-history").replaceChildren();
      });
  }

  function activeFilters() {
    return filters.filter(function (filter) { return filter.dataset.filter !== "all" && filter.getAttribute("aria-pressed") === "true"; })
      .map(function (filter) { return filter.dataset.filter; });
  }

  function matches(row, query, active) {
    if (query && !row.dataset.search.includes(query)) return false;
    if (repoFilter && repoFilter.value && row.dataset.repository !== repoFilter.value) return false;
    if (active.includes("recent") && Date.now() - Date.parse(row.dataset.updated) > 7 * 24 * 60 * 60 * 1000) return false;
    if (active.includes("multi") && Number(row.dataset.versionCount) < 2) return false;
    if (active.includes("disabled") && row.dataset.disabled !== "true") return false;
    if (active.includes("missing") && row.dataset.provenanceRecorded === "true") return false;
    return true;
  }

  function applyFilters() {
    var query = search.value.trim().toLocaleLowerCase();
    var active = activeFilters();
    var visible = [];
    rows.forEach(function (row) {
      var show = matches(row, query, active);
      row.hidden = !show;
      if (show) visible.push(row);
    });
    resultCount.textContent = visible.length + (visible.length === 1 ? " draft" : " drafts");
    emptyResults.hidden = visible.length !== 0;
    if (!visible.some(function (row) { return row.dataset.draftId === selectedId; })) {
      if (visible[0]) populateSummary(visible[0]);
      else document.getElementById("draft-detail").classList.add("empty");
    } else {
      document.getElementById("draft-detail").classList.remove("empty");
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

  filters.forEach(function (filter) {
    filter.addEventListener("click", function () {
      if (filter.dataset.filter === "all") {
        filters.forEach(function (candidate) { candidate.setAttribute("aria-pressed", String(candidate === filter)); });
      } else {
        filter.setAttribute("aria-pressed", String(filter.getAttribute("aria-pressed") !== "true"));
        filters.find(function (candidate) { return candidate.dataset.filter === "all"; }).setAttribute("aria-pressed", "false");
        if (activeFilters().length === 0) filters.find(function (candidate) { return candidate.dataset.filter === "all"; }).setAttribute("aria-pressed", "true");
      }
      applyFilters();
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
    row.querySelector(".updated").textContent = relativeTime(row.dataset.updated);
    row.addEventListener("click", function (event) {
      if (event.target.closest("a, button, details, summary")) return;
      populateSummary(row);
    });
    row.addEventListener("keydown", function (event) {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        populateSummary(row);
      }
    });
  });

  document.addEventListener("click", function (event) {
    document.querySelectorAll(".row-menu[open]").forEach(function (menu) {
      if (!menu.contains(event.target)) menu.removeAttribute("open");
    });
  });

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
          var row = rows.find(function (candidate) { return candidate.dataset.draftId === pending.id; });
          if (row) {
            rows = rows.filter(function (candidate) { return candidate !== row; });
            row.remove();
          }
          dialog.close();
          showToast(pending.action === "purge" ? "Draft permanently deleted." : "Draft pruned.", false);
          pending = null;
          applyFilters();
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

  if (rows[0]) populateSummary(rows[0]);
  applySort();
})();
