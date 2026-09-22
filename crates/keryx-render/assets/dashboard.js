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

  // Tags are dashboard metadata. The modal owns its target and survives snapshots.
  var catalogueNode = document.getElementById("tag-catalogue");
  var catalogue = catalogueNode ? JSON.parse(catalogueNode.dataset.tags) : [];
  var selectedTags = new Set();
  var untagged = false;
  var tagDialog = document.getElementById("tag-dialog");
  var tagInput = document.getElementById("tag-input");
  var tagModal = null;
  var tagTimer = null;
  var tagMenu = document.getElementById("tag-filter");
  var removingTags = new Set();

  function rowTags(row) { return JSON.parse(row.dataset.tags || "[]"); }
  function canonicalTag(value) {
    if (!/^[a-zA-Z0-9 -]*$/.test(value)) return null;
    var name = value.trim().replace(/ +/g, " ").toLowerCase();
    return name.length > 0 && name.length <= 32 ? name : null;
  }
  function tagButton(text, label, action) {
    var button = document.createElement("button");
    button.type = "button";
    button.className = "tag-chip";
    button.textContent = text;
    button.setAttribute("aria-label", label);
    button.addEventListener("click", action);
    return button;
  }
  function clearTags() {
    selectedTags.clear(); untagged = false; applyFilters(); syncUrl();
    document.getElementById("tag-filter-summary").focus();
  }
  function chooseTag(id) {
    untagged = false; selectedTags.add(id); applyFilters(); syncUrl();
  }
  function matchesTags(row) {
    var tags = rowTags(row);
    if (untagged) return tags.length === 0;
    return !selectedTags.size || tags.some(function (tag) { return selectedTags.has(tag.id); });
  }
  function renderTagFilters(query) {
    if (!managementEnabled) return;
    var counts = new Map();
    var empty = 0;
    rows.filter(function (row) { return matches(row, query); }).forEach(function (row) {
      var tags = rowTags(row);
      if (!tags.length) empty += 1;
      tags.forEach(function (tag) { counts.set(tag.id, (counts.get(tag.id) || 0) + 1); });
    });
    var options = document.getElementById("tag-filter-options");
    var focusedId = document.activeElement && document.activeElement.dataset.tagChoice;
    options.replaceChildren();
    var filterQuery = document.getElementById("tag-filter-search").value.trim().toLowerCase();
    catalogue.filter(function (tag) { return tag.name.includes(filterQuery); }).forEach(function (tag) {
      var label = document.createElement("label");
      var box = document.createElement("input");
      box.type = "checkbox"; box.checked = selectedTags.has(tag.id); box.dataset.tagChoice = tag.id;
      box.addEventListener("change", function () {
        untagged = false;
        if (box.checked) selectedTags.add(tag.id); else selectedTags.delete(tag.id);
        applyFilters(); syncUrl();
      });
      label.append(box, document.createTextNode(" " + tag.name + " (" + (counts.get(tag.id) || 0) + ")"));
      options.append(label);
      if (focusedId === tag.id) box.focus();
    });
    document.getElementById("tag-untagged").checked = untagged;
    document.getElementById("untagged-count").textContent = "(" + empty + ")";
    document.getElementById("tag-filter-summary").textContent = untagged ? "Tags: untagged" : (selectedTags.size ? "Tags: " + selectedTags.size + " selected" : "Tags: all");
    var chips = document.getElementById("tag-selections");
    chips.replaceChildren();
    selectedTags.forEach(function (id) {
      var tag = catalogue.find(function (tag) { return tag.id === id; });
      var name = tag ? tag.name : "Missing tag " + id;
      chips.append(tagButton(name + " ×", "Clear filter " + name, function () {
        selectedTags.delete(id); applyFilters(); syncUrl();
        document.getElementById("tag-filter-summary").focus();
      }));
    });
    if (untagged) chips.append(tagButton("Untagged ×", "Clear untagged filter", clearTags));
    if (selectedTags.size || untagged) chips.append(tagButton("Clear all", "Clear all tag filters", clearTags));
    document.getElementById("clear-empty-tags").hidden = emptyResults.hidden || (!selectedTags.size && !untagged);
    document.getElementById("tag-sort-help").hidden = sort.value !== "tag";
  }
  function renderDetailTags(row) {
    var container = document.getElementById("detail-tags");
    if (!container) return;
    container.replaceChildren();
    rowTags(row).forEach(function (tag) {
      var button = tagButton(tag.name + " ×", "Remove tag " + tag.name, function () {
        var key = row.dataset.draftId + "/" + tag.id;
        if (removingTags.has(key)) return;
        removingTags.add(key); button.disabled = true;
        tagRequest(row.dataset.draftId, tag.id).then(function () {
          refreshDashboard();
        }).catch(function (error) { showToast(error.message, true); }).finally(function () {
          removingTags.delete(key);
          var current = findRow(selectedId);
          if (current) renderDetailTags(current);
        });
      });
      button.disabled = removingTags.has(row.dataset.draftId + "/" + tag.id);
      container.append(button);
    });
    var add = tagButton("+ Add tag", "Add tag", function () { openTagDialog(row); });
    add.id = "add-tag"; add.className = "button";
    container.append(add);
  }
  function tagRequest(draftId, tagId, name) {
    return fetch("/api/dashboard/drafts/" + encodeURIComponent(draftId) + "/tags" + (tagId ? "/" + encodeURIComponent(tagId) : ""), {
      method: tagId ? "DELETE" : "POST",
      headers: { Accept: "application/json", "Content-Type": "application/json" },
      body: tagId ? undefined : JSON.stringify({ name: name })
    }).then(function (response) {
      return response.json().then(function (body) {
        if (!response.ok) throw new Error(body.error || "Tags could not be saved.");
        return body;
      });
    });
  }
  function cancelTagTimer() { clearTimeout(tagTimer); tagTimer = null; }
  function closeTagDialog() {
    cancelTagTimer(); tagModal = null; tagDialog.close();
    var trigger = document.getElementById("add-tag");
    var detail = document.getElementById("draft-detail");
    if (!trigger || detail.classList.contains("empty")) trigger = search;
    trigger.focus();
  }
  function openTagDialog(row) {
    cancelTagTimer();
    tagModal = { draftId: row.dataset.draftId, pending: false, composing: false, waiting: false, choices: [], active: -1 };
    tagInput.value = ""; tagInput.disabled = false;
    document.getElementById("tag-target").textContent = "To " + row.dataset.title;
    document.getElementById("tag-error").textContent = "";
    reconcileTagSuggestions(); tagDialog.showModal(); tagInput.focus();
  }
  function reconcileTagSuggestions() {
    if (!tagModal) return;
    var row = findRow(tagModal.draftId);
    if (!row) { closeTagDialog(); showToast("The draft is no longer available for tagging.", true); return; }
    if (tagModal.pending || tagModal.waiting || tagModal.composing) return;
    var previous = tagModal.choices[tagModal.active];
    var assigned = new Set(rowTags(row).map(function (tag) { return tag.id; }));
    var query = tagInput.value.trim().replace(/ +/g, " ").toLowerCase();
    function rank(tag) { return tag.name === query ? 0 : tag.name.startsWith(query) ? 1 : 2; }
    var choices = catalogue.filter(function (tag) { return tag.name.includes(query); }).sort(function (a, b) {
      return rank(a) - rank(b) || a.name.localeCompare(b.name);
    }).slice(0, 8).map(function (tag) { return { id: tag.id, name: tag.name, added: assigned.has(tag.id) }; });
    var canonical = canonicalTag(tagInput.value);
    if (canonical && !catalogue.some(function (tag) { return tag.name === canonical; })) {
      choices = choices.slice(0, 7);
      choices.push({ id: "create:" + canonical, name: canonical, create: true });
    }
    tagModal.choices = choices;
    tagModal.active = previous ? choices.findIndex(function (choice) { return choice.id === previous.id && !choice.added; }) : -1;
    if (tagModal.active < 0) tagModal.active = choices.findIndex(function (choice) { return !choice.added; });
    renderTagSuggestions();
  }
  function renderTagSuggestions() {
    var list = document.getElementById("tag-suggestions");
    list.replaceChildren(); tagInput.removeAttribute("aria-activedescendant");
    if (!tagModal) return;
    var blocked = tagModal.waiting || tagModal.composing || tagModal.pending;
    tagInput.setAttribute("aria-expanded", String(!blocked && tagModal.choices.length > 0));
    document.getElementById("tag-status").textContent = tagModal.pending ? "Saving…" : blocked ? "Updating suggestions…" : tagModal.choices.length + " options";
    if (blocked) return;
    tagModal.choices.forEach(function (choice, index) {
      var option = document.createElement("div");
      option.id = "tag-option-" + index; option.setAttribute("role", "option");
      option.setAttribute("aria-selected", String(index === tagModal.active));
      option.setAttribute("aria-disabled", String(Boolean(choice.added)));
      option.textContent = choice.create ? '+ Create "' + choice.name + '"' : choice.name + (choice.added ? " · Added" : "");
      option.addEventListener("mousedown", function (event) { event.preventDefault(); });
      option.addEventListener("click", function () { submitTag(index); });
      list.append(option);
      if (index === tagModal.active) tagInput.setAttribute("aria-activedescendant", option.id);
    });
  }
  function submitTag(index) {
    var session = tagModal;
    if (!session || session.pending || session.waiting || session.composing) return;
    var choice = session.choices[index];
    if (!choice || choice.added) return;
    session.pending = true; tagInput.disabled = true;
    document.getElementById("tag-error").textContent = ""; renderTagSuggestions();
    tagRequest(session.draftId, null, choice.name).then(function () {
      if (tagModal === session) closeTagDialog();
      refreshDashboard();
    }).catch(function (error) {
      if (tagModal !== session) return;
      session.pending = false; tagInput.disabled = false;
      document.getElementById("tag-error").textContent = error.message;
      reconcileTagSuggestions(); tagInput.focus();
    });
  }
  function debounceTags() {
    if (!tagModal) return;
    cancelTagTimer(); tagModal.waiting = true; tagModal.active = -1; renderTagSuggestions();
    if (tagModal.composing) return;
    tagTimer = setTimeout(function () {
      tagTimer = null;
      if (!tagModal) return;
      tagModal.waiting = false; reconcileTagSuggestions();
    }, 150);
  }
  if (managementEnabled) {
    tagInput.addEventListener("input", debounceTags);
    tagInput.addEventListener("compositionstart", function () { tagModal.composing = true; debounceTags(); });
    tagInput.addEventListener("compositionend", function () { tagModal.composing = false; debounceTags(); });
    tagInput.addEventListener("keydown", function (event) {
      if (event.isComposing || !tagModal || tagModal.composing) return;
      if (event.key === "Enter") { event.preventDefault(); submitTag(tagModal.active); }
      if (!["ArrowDown", "ArrowUp"].includes(event.key) || tagModal.waiting || tagModal.pending) return;
      event.preventDefault();
      var choices = tagModal.choices;
      for (var step = 1; step <= choices.length; step += 1) {
        var index = (tagModal.active + (event.key === "ArrowDown" ? step : -step) + choices.length) % choices.length;
        if (!choices[index].added) { tagModal.active = index; break; }
      }
      renderTagSuggestions();
    });
    tagDialog.addEventListener("keydown", function (event) {
      if (event.key !== "Tab") return;
      var cancel = document.getElementById("tag-cancel");
      if (tagInput.disabled || (event.shiftKey && document.activeElement === tagInput)) {
        event.preventDefault(); cancel.focus();
      } else if (!event.shiftKey && document.activeElement === cancel) {
        event.preventDefault(); tagInput.focus();
      }
    });
    tagDialog.addEventListener("cancel", function (event) { event.preventDefault(); closeTagDialog(); });
    document.getElementById("tag-cancel").addEventListener("click", closeTagDialog);
    document.getElementById("tag-filter-search").addEventListener("input", function () { renderTagFilters(search.value.trim().toLowerCase()); });
    document.getElementById("tag-untagged").addEventListener("change", function (event) {
      untagged = event.target.checked; selectedTags.clear(); applyFilters(); syncUrl();
    });
    document.querySelectorAll("[data-clear-tags]").forEach(function (button) { button.addEventListener("click", clearTags); });
    function closeTagFilter() { tagMenu.open = false; document.getElementById("tag-filter-summary").focus(); }
    document.getElementById("tag-filter-done").addEventListener("click", closeTagFilter);
    tagMenu.addEventListener("keydown", function (event) { if (event.key === "Escape") { event.preventDefault(); closeTagFilter(); } });
    document.addEventListener("click", function (event) { if (!tagMenu.contains(event.target)) tagMenu.open = false; });
    document.addEventListener("click", function (event) {
      var chip = event.target.closest("[data-filter-tag]");
      if (chip) { event.stopPropagation(); chooseTag(chip.dataset.filterTag); }
    });
  }

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

    renderDetailTags(row);
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
      var show = matches(row, query) && matchesTags(row);
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
        selectedId = null;
      }
    } else {
      detail.classList.remove("empty");
    }
    renderTagFilters(query);
  }

  function applySort() {
    var direction = sort.value;
    rows.sort(function (left, right) {
      if (direction === "tag") {
        var a = rowTags(left)[0]; var b = rowTags(right)[0];
        return (!a - !b) || (a && b ? a.name.localeCompare(b.name) : 0) || left.dataset.title.localeCompare(right.dataset.title) || left.dataset.draftId.localeCompare(right.dataset.draftId);
      }
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
    if (search.value) params.set("search", search.value);
    if (repoFilter && repoFilter.value) params.set("repo", repoFilter.value);
    if (sort.value !== "updated") params.set("sort", sort.value);
    selectedTags.forEach(function (id) { params.append("tag", id); });
    if (untagged) params.set("untagged", "1");
    history.replaceState(null, "", "/?" + params.toString());
  }

  function findRow(draftId) {
    return rows.find(function (candidate) { return candidate.dataset.draftId === draftId; });
  }

  function bindRow(row) {
    syncAvailability(row);
    renderRowTime(row);
    row.addEventListener("click", function (event) {
      if (event.target.closest("a, button, details, summary")) return;
      populateSummary(row);
      syncUrl();
    });
    row.addEventListener("keydown", function (event) {
      if (event.target !== row) return;
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        populateSummary(row);
        syncUrl();
      }
    });
    if (managementEnabled) {
      row.querySelectorAll("[data-action]").forEach(function (button) {
        button.addEventListener("click", function () { openPruneDialog(button); });
      });
    }
  }

  function syncRepositoryFilter() {
    if (!repoFilter) return;
    var selected = repoFilter.value;
    var repositories = Array.from(new Set(rows
      .filter(function (row) { return !row.querySelector(".source.missing"); })
      .map(function (row) { return row.dataset.repository; })))
      .sort(function (left, right) { return left.localeCompare(right); });
    repoFilter.replaceChildren();
    var all = document.createElement("option");
    all.value = "";
    all.textContent = "All repositories";
    repoFilter.append(all);
    if (selected && !repositories.includes(selected)) repositories.push(selected);
    repositories.forEach(function (repository) {
      var option = document.createElement("option");
      option.value = repository;
      option.textContent = repository;
      repoFilter.append(option);
    });
    repoFilter.value = repositories.includes(selected) ? selected : "";
  }

  tabs.forEach(function (tab) {
    tab.addEventListener("click", function () {
      setView(tab.dataset.view);
      syncUrl();
    });
  });
  search.addEventListener("input", function () { applyFilters(); syncUrl(); });
  if (repoFilter) repoFilter.addEventListener("change", function () { applyFilters(); syncUrl(); });
  sort.addEventListener("change", function () { applySort(); syncUrl(); });
  document.addEventListener("keydown", function (event) {
    if (event.key === "/" && !event.target.closest("input, textarea, select, dialog")) {
      event.preventDefault();
      search.focus();
    }
  });

  rows.forEach(bindRow);

  document.addEventListener("click", function (event) {
    document.querySelectorAll(".row-menu[open], .notify-menu[open]").forEach(function (menu) {
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

    var bindDetailActions = function () {
      document.querySelectorAll("[data-availability-action]").forEach(function (button) {
        if (button.dataset.bound === "true") return;
        button.dataset.bound = "true";
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
    };
    bindDetailActions();
  } else {
    var bindDetailActions = function () {};
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

  // --- installable app --------------------------------------------------
  // The browser's real origin decides: the worker registers only in a secure
  // context and the install action appears only when the browser offers it.
  // Plain HTTP keeps the ordinary dashboard.

  var installButton = document.getElementById("install-app");
  var installPrompt = null;
  if (window.isSecureContext && "serviceWorker" in navigator) {
    navigator.serviceWorker.register("/sw.js").catch(function (error) {
      console.warn("Keryx service worker unavailable:", error);
    });
  }
  window.addEventListener("beforeinstallprompt", function (event) {
    event.preventDefault();
    installPrompt = event;
    installButton.hidden = false;
  });
  installButton.addEventListener("click", function () {
    if (!installPrompt) return;
    installPrompt.prompt();
    installPrompt.userChoice.finally(function () {
      installPrompt = null;
      installButton.hidden = true;
    });
  });
  window.addEventListener("appinstalled", function () {
    installButton.hidden = true;
    showToast("Keryx installed.", false);
  });

  // --- push notifications -----------------------------------------------
  // The subscription lives in the browser; the server stores its keys and
  // event preferences by endpoint. Denied or unsupported notifications never
  // affect snoozing: the in-page wake timer above still runs.

  var notifyMenu = document.getElementById("notify-menu");
  var pushSupported = Boolean(notifyMenu) && window.isSecureContext && "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;
  if (pushSupported) {
    var notifySummary = document.getElementById("notify-summary");
    var notifyState = document.getElementById("notify-state");
    var notifyEnable = document.getElementById("notify-enable");
    var notifyDisable = document.getElementById("notify-disable");
    var eventBoxes = Array.from(document.querySelectorAll("[data-event]"));
    notifyMenu.hidden = false;

    var base64UrlToBytes = function (value) {
      var padded = (value + "=".repeat((4 - (value.length % 4)) % 4)).replace(/-/g, "+").replace(/_/g, "/");
      var raw = atob(padded);
      var bytes = new Uint8Array(raw.length);
      for (var index = 0; index < raw.length; index += 1) bytes[index] = raw.charCodeAt(index);
      return bytes;
    };
    var selectedEvents = function () {
      return eventBoxes.filter(function (box) { return box.checked; }).map(function (box) { return box.dataset.event; });
    };
    var pushRequest = function (method, body) {
      return fetch("/api/push/subscriptions", {
        method: method,
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify(body)
      }).then(function (response) {
        return response.json().catch(function () { return {}; }).then(function (payload) {
          if (!response.ok) {
            throw new Error(response.status === 401 ? "API key required. Use the CLI for this action." : (payload.error || "Notification settings could not be saved."));
          }
          return payload;
        });
      });
    };
    var renderNotifyState = function (subscription) {
      var on = Boolean(subscription);
      notifySummary.textContent = on ? "Notifications on" : "Notifications off";
      notifySummary.classList.toggle("on", on);
      notifyEnable.hidden = on;
      notifyDisable.hidden = !on;
      eventBoxes.forEach(function (box) {
        if (subscription) box.checked = subscription.events.includes(box.dataset.event);
        box.disabled = !on;
      });
      if (on) notifyState.textContent = "This device gets draft activity even while Keryx is closed.";
      else if (Notification.permission === "denied") notifyState.textContent = "Notifications are blocked for this site in the browser. Snoozing still works, and wakes show here while the dashboard is open.";
      else notifyState.textContent = "Get a notification when a plan is published, revised, wakes, or changes availability, even with Keryx closed.";
    };
    var currentSubscription = function () {
      return navigator.serviceWorker.ready.then(function (registration) { return registration.pushManager.getSubscription(); });
    };
    var saveSubscription = function (subscription, events) {
      var json = subscription.toJSON();
      return pushRequest("PUT", { endpoint: json.endpoint, keys: json.keys, events: events }).then(function (body) {
        renderNotifyState(body.subscription);
        return body.subscription;
      });
    };

    renderNotifyState(null);
    currentSubscription()
      .then(function (subscription) { if (subscription) return saveSubscription(subscription, null); })
      .catch(function () { renderNotifyState(null); });

    notifyEnable.addEventListener("click", function () {
      notifyEnable.disabled = true;
      Notification.requestPermission()
        .then(function (permission) {
          if (permission !== "granted") {
            renderNotifyState(null);
            return null;
          }
          return fetch("/api/push/vapid", { headers: { Accept: "application/json" } })
            .then(function (response) { return response.json(); })
            .then(function (body) {
              return navigator.serviceWorker.ready.then(function (registration) {
                return registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: base64UrlToBytes(body.publicKey) });
              });
            })
            .then(function (subscription) { return saveSubscription(subscription, selectedEvents()); })
            .then(function () { showToast("Notifications enabled on this device.", false); });
        })
        .catch(function (error) { showToast(error.message || "Notifications could not be enabled.", true); })
        .finally(function () { notifyEnable.disabled = false; });
    });
    notifyDisable.addEventListener("click", function () {
      currentSubscription()
        .then(function (subscription) {
          if (!subscription) return null;
          return pushRequest("DELETE", { endpoint: subscription.endpoint }).then(function () { return subscription.unsubscribe(); });
        })
        .then(function () {
          renderNotifyState(null);
          showToast("Notifications turned off on this device.", false);
        })
        .catch(function (error) { showToast(error.message, true); });
    });
    eventBoxes.forEach(function (box) {
      box.addEventListener("change", function () {
        currentSubscription()
          .then(function (subscription) { if (subscription) return saveSubscription(subscription, selectedEvents()); })
          .catch(function (error) { showToast(error.message, true); });
      });
    });
  }

  // --- realtime dashboard ----------------------------------------------
  // The event stream is an invalidation signal, not a second data source.
  // Each signal fetches one authoritative server-rendered snapshot.

  var refreshInFlight = false;
  var refreshPending = false;

  function refreshDashboard() {
    if (refreshInFlight) {
      refreshPending = true;
      return;
    }
    refreshInFlight = true;
    var query = selectedId ? "?selected=" + encodeURIComponent(selectedId) : "";
    fetch("/api/dashboard/snapshot" + query, { headers: { Accept: "application/json" } })
      .then(function (response) {
        if (!response.ok) throw new Error("Dashboard snapshot returned HTTP " + response.status);
        return response.json();
      })
      .then(function (snapshot) {
        if (typeof snapshot.rows !== "string" || typeof snapshot.detail !== "string") {
          throw new Error("Dashboard snapshot was malformed");
        }
        if (managementEnabled) catalogue = snapshot.tags || [];
        var focused = document.activeElement;
        var detailHadFocus = document.getElementById("draft-detail").contains(focused);
        var focusId = focused && focused.id;
        tbody.innerHTML = snapshot.rows;
        var detail = document.getElementById("draft-detail");
        detail.outerHTML = snapshot.detail;
        rows = Array.from(tbody.querySelectorAll(".draft-row"));
        rows.forEach(bindRow);
        syncRepositoryFilter();
        reconcileTagSuggestions();
        bindDetailActions();
        applySort();

        var selected = findRow(selectedId);
        if (selected && !selected.hidden) {
          populateSummary(selected);
        }
        if (detailHadFocus && !tagModal) {
          var focusTarget = focusId && document.getElementById(focusId);
          (selected && !selected.hidden && focusTarget ? focusTarget : search).focus();
        }
        syncUrl();
        scheduleWake();
      })
      .catch(function (error) {
        console.warn("Keryx realtime refresh failed:", error);
      })
      .finally(function () {
        refreshInFlight = false;
        if (refreshPending) {
          refreshPending = false;
          refreshDashboard();
        }
      });
  }

  if ("EventSource" in window) {
    var dashboardEvents = new EventSource("/api/dashboard/events");
    dashboardEvents.addEventListener("dashboard", refreshDashboard);
  }

  // --- deep links -------------------------------------------------------
  // /?draft=<id>&view=<availability> selects a tab and a draft. A draft that
  // lives in another tab wins over the view parameter.

  var params = new URLSearchParams(location.search);
  search.value = params.get("search") || "";
  if (repoFilter && params.get("repo")) {
    var repositoryOption = document.createElement("option");
    repositoryOption.value = params.get("repo"); repositoryOption.textContent = params.get("repo");
    repoFilter.append(repositoryOption); repoFilter.value = params.get("repo");
  }
  if (["updated", "oldest", "title", "versions", "tag"].includes(params.get("sort"))) sort.value = params.get("sort");
  if (managementEnabled) {
    untagged = params.get("untagged") === "1";
    if (!untagged) selectedTags = new Set(params.getAll("tag"));
  }
  var linkedRow = params.get("draft") ? findRow(params.get("draft")) : null;
  var linkedView = params.get("view") || "active";
  if (linkedRow) linkedView = linkedRow.dataset.availability;
  applySort();
  setView(linkedView);
  if (linkedRow && !linkedRow.hidden) {
    populateSummary(linkedRow);
    linkedRow.scrollIntoView({ block: "nearest" });
  }
  syncUrl();
  scheduleWake();
})();
