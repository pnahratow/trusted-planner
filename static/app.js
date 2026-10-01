// Progressive enhancement only. Drag & drop (phase 5) and the SSE wiring
// (phase 6) land here; for now this keeps a column re-render from interrupting
// whoever is typing in it.

(function () {
  "use strict";

  var body = document.body;

  // Which add-form (if any) started the request currently in flight. Without
  // this we cannot tell "the column re-rendered under me while I was typing"
  // — where the draft must be put back — from "I just submitted this form",
  // where the server's cleared field is the correct result and restoring the
  // text would look like the add silently failed.
  var submittedKey = null;
  var pending = null;

  body.addEventListener("htmx:beforeRequest", function (evt) {
    var elt = evt.detail && evt.detail.elt;
    var form = elt && elt.closest ? elt.closest(".add-task") : null;
    var input = form ? form.querySelector(".add-input") : null;
    submittedKey = input ? input.dataset.column : null;
  });

  body.addEventListener("htmx:beforeSwap", function () {
    var el = document.activeElement;
    if (el && el.classList && el.classList.contains("add-input")) {
      pending = {
        key: el.dataset.column,
        value: el.value,
        caret: el.selectionStart,
        // True when this swap is the answer to this very field's submission.
        submitted: submittedKey !== null && submittedKey === el.dataset.column,
      };
    } else {
      pending = null;
    }
  });

  body.addEventListener("htmx:afterSwap", function () {
    var p = pending;
    pending = null;
    if (!p) return;

    var next = document.querySelector('.add-input[data-column="' + p.key + '"]');
    if (!next) return;

    // Keep the cursor where it was either way, so you can keep typing.
    next.focus();

    if (p.submitted) {
      // The form node is preserved across the swap, so it still holds the text
      // that was just submitted; clear it ourselves.
      next.value = "";
      return;
    }
    if (p.value && next.value === "") {
      next.value = p.value;
      var at = Math.min(p.caret || 0, next.value.length);
      next.setSelectionRange(at, at);
    }
  });

  // --------------------------------------------------------------- undo
  //
  // A delete fills the bar out of band; this clears it again after a while so
  // it does not sit there for the rest of the session. Undoing empties it too,
  // through the same out-of-band swap.
  var UNDO_MS = 9000;
  var undoTimer = null;

  body.addEventListener("htmx:afterSwap", function (evt) {
    if (!evt.target || evt.target.id !== "undo-bar") return;
    clearTimeout(undoTimer);
    if (!evt.target.children.length) return; // an undo just emptied it
    undoTimer = setTimeout(function () {
      var bar = document.getElementById("undo-bar");
      if (bar) bar.innerHTML = "";
    }, UNDO_MS);
  });

  // --------------------------------------------------- click away to save
  //
  // An open editor is closed by clicking anything that is not part of it, by
  // tabbing out of it, or by pressing Escape. There is no Save button: leaving
  // is the instruction, the way the settings page works — the change *is* what
  // you meant, and a button to confirm it asks a question that has already
  // been answered.
  //
  // Escape is the exception, and the only way out that discards: closing by
  // leaving cannot also mean "forget that", so the one deliberate keystroke
  // does.
  //
  // Both endings click the editor's own button rather than re-issuing its
  // request here, so the URLs and the targets stay declared in the markup, in
  // one place, and closing can never drift from what the buttons do.
  function closingAlready(editor) {
    if (editor.dataset.closing) return true; // a stray second event must not re-fire
    editor.dataset.closing = "1";
    return false;
  }

  function cancelEditor(editor) {
    var button = editor.querySelector("[data-cancel-edit]");
    if (!button || closingAlready(editor)) return;
    button.click();
  }

  // Nothing typed, nothing switched: the save would be a write that changes no
  // field, and writes are not free here — one bumps the version, which makes
  // every other editor open on this task stale, and tells every other browser
  // to re-fetch the column. Opening a row to read its notes and clicking away
  // must cost nothing, so that closes by the same path as Cancel.
  function isDirty(editor) {
    var fields = editor.querySelectorAll("input, textarea");
    for (var i = 0; i < fields.length; i++) {
      var f = fields[i];
      if (f.type === "radio" || f.type === "checkbox") {
        if (f.checked !== f.defaultChecked) return true;
      } else if (f.value !== f.defaultValue) {
        return true;
      }
    }
    return false;
  }

  function closeEditor(editor) {
    // A conflict is the one moment the editor is asking something rather than
    // recording something, and it puts two buttons on screen to ask it with.
    // Clicking elsewhere is not an answer, so it is not taken as one.
    if (editor.querySelector(".conflict")) return;

    var save = editor.querySelector("[data-save-edit]");
    var title = editor.querySelector(".edit-title");
    // An empty title is refused by the server, which would leave an editor
    // that will not close. Emptying a title and walking away is not an
    // instruction to name it nothing; it is leaving it as it was. Anything
    // else unexpected falls through to Cancel for the same reason: whatever
    // else happens, the editor closes.
    if (save && isDirty(editor) && title && title.value.trim()) {
      if (!closingAlready(editor)) save.click();
      return;
    }
    cancelEditor(editor);
  }

  function closeEditorsOutside(target) {
    var open = document.querySelectorAll(".task-editing");
    for (var i = 0; i < open.length; i++) {
      if (!open[i].contains(target)) closeEditor(open[i]);
    }
  }

  // The day panel opened from a month cell's "+N more". Emptying the container
  // hides it, because it is styled `:empty { display: none }` — no state to
  // keep and nothing to get out of step with.
  function closeDayPanel() {
    var panel = document.getElementById("day-panel");
    if (panel) panel.innerHTML = "";
  }

  document.addEventListener("click", function (evt) {
    if (evt.target.closest && evt.target.closest("[data-close-panel]")) {
      closeDayPanel();
      return;
    }
    // Clicking the empty space in a column starts a new task there. Now that
    // columns are full height that space is most of the screen, and it read as
    // dead area.
    if (evt.target.classList && evt.target.classList.contains("tasks")) {
      var input = evt.target.parentElement.querySelector(".add-input");
      if (input) input.focus();
    }
    // Opening another editor is handled by that row's own request; closing
    // this one alongside it is exactly the intent.
    closeEditorsOutside(evt.target);
  });

  // Leaving by keyboard. `relatedTarget` must be a real element: a focusout
  // with nothing receiving focus is the window itself losing it — switching to
  // another app is not leaving the editor, and must not save and close it.
  document.addEventListener("focusout", function (evt) {
    var editor = evt.target.closest ? evt.target.closest(".task-editing") : null;
    if (!editor) return;
    var to = evt.relatedTarget;
    if (to && !editor.contains(to)) closeEditor(editor);
  });

  document.addEventListener("keydown", function (evt) {
    if (evt.key !== "Escape") return;
    var editor = evt.target.closest ? evt.target.closest(".task-editing") : null;
    if (editor) {
      cancelEditor(editor);
      return;
    }
    closeEditorsOutside(document.body);
    closeDayPanel();
  });

  // ------------------------------------------------------------ drag & drop
  //
  // Sortable moves the row optimistically so the drag feels immediate; the
  // server then re-renders the column and its answer wins. Nothing here tries
  // to keep a local model in sync.
  function initSortable(root) {
    if (!window.Sortable) return;
    var lists = (root || document).querySelectorAll(".tasks");
    for (var i = 0; i < lists.length; i++) {
      var ul = lists[i];
      if (window.Sortable.get(ul)) continue; // already wired
      window.Sortable.create(ul, {
        group: "tasks",
        draggable: ".task",
        filter: ".empty, .task-editing", // placeholders and open editors don't drag
        animation: 120,
        // A touch drag and a scroll open with the same gesture, so the drag has to
        // prove itself: the finger must rest on the row before it picks up, and a
        // finger that travels more than a few pixels in that time was scrolling
        // past. Without this every scroll that starts on a task reorders it.
        // `delayOnTouchOnly` keeps the mouse immediate, where there is no
        // ambiguity to resolve.
        delay: 200,
        delayOnTouchOnly: true,
        touchStartThreshold: 5,
        ghostClass: "drag-ghost",
        chosenClass: "drag-chosen",
        onEnd: onDrop,
      });
    }
  }

  function onDrop(evt) {
    var item = evt.item;
    if (evt.from === evt.to && evt.oldIndex === evt.newIndex) return; // no-op

    var column = evt.to.closest(".column");
    if (!column) return;
    var key = column.dataset.key;
    var id = item.dataset.taskId;
    if (!key || !id) return;

    // Name the neighbour rather than an index: with completed tasks sunk to
    // the bottom, the position on screen is not the position in the table.
    var prev = item.previousElementSibling;
    while (prev && !prev.classList.contains("task")) {
      prev = prev.previousElementSibling;
    }

    var values = { key: key };
    if (prev && prev.dataset.taskId) values.after = prev.dataset.taskId;

    window.htmx.ajax("POST", "/task/" + id + "/move", {
      target: "#col-" + key,
      swap: "outerHTML",
      values: values,
    });
  }

  initSortable(document);
  body.addEventListener("htmx:afterSwap", function (evt) {
    initSortable(evt.target && evt.target.querySelectorAll ? evt.target : document);
    // A swapped-in column is a new node, so re-scan the document as well.
    initSortable(document);
  });

  // Closing an editor can unblock a column that was waiting to catch up — but
  // only once htmx has finished settling. During `afterSwap` the replaced
  // element is still momentarily in the document, so a column would still look
  // busy and the deferred refresh would be skipped.
  body.addEventListener("htmx:afterSettle", function () {
    flushStale();
  });

  // --------------------------------------------------------- live updates
  //
  // The server is asked "what changed since sequence N"; the client answers by
  // re-fetching those columns through the ordinary fragment route. Coarse
  // invalidations mean the polled path and the fetched path are the same code
  // and cannot disagree.
  //
  // Short polls rather than a held-open stream: see the note at the top of
  // src/events.rs — an SSE connection per tab exhausts the browser's six
  // connections per origin and freezes the whole app at five tabs.
  //
  // Ten seconds, not three. A longer interval cannot lose a change, only show
  // it later: the client quotes the last seq it saw, so the next tick returns
  // everything it missed. Three seconds was 28,800 requests a day for every
  // tab left open, nearly all of them answering "nothing changed".
  var POLL_MS = 10000;
  var live = document.getElementById("live");

  // Only an open editor blocks a refresh. Swapping the column out from under
  // one would destroy an edit in progress and the version it is checked
  // against, and no re-render can put that back.
  //
  // Typing in the add box is deliberately *not* a blocker: the swap guard
  // above restores the draft and the caret, so the column stays current
  // instead of waiting for the person to click elsewhere.
  function isBusy(col) {
    return !!col.querySelector(".task-editing");
  }

  function refreshColumn(col) {
    if (isBusy(col)) {
      col.dataset.stale = "1";
      return;
    }
    delete col.dataset.stale;
    window.htmx.trigger(col, "refresh-column");
  }

  // ----------------------------------------------------------- day rollover
  //
  // A page carries the date it was rendered for, and every poll answers with
  // the date the server is in. When they diverge the render is simply of the
  // wrong day: the today-highlight is on yesterday's column, and the overdue
  // sweep — which runs on page load, not on a timer — has not run for the new
  // day. Neither is something re-fetching a column can repair, because the
  // poll only re-fetches columns that *changed*, and midnight changes nothing.
  //
  // So the answer is a reload, which re-runs the sweep and re-renders every
  // column against the new date. On a URL ending `/today` it also re-anchors
  // the window, which is what keeps a 24/7 display board on the current week
  // instead of the week it was switched on in.
  //
  // The server's date, not the browser's: the app answers "today" in
  // PLANNER_TZ, and a display on a host set to UTC would otherwise roll over
  // at the wrong moment.
  var dayChanged = false;

  function reloadForNewDay() {
    // An open editor outranks this. The reload would destroy an edit in
    // progress, and a day-old highlight is a far smaller problem than lost
    // text; flushStale picks it up again when the editor closes.
    if (document.querySelector(".task-editing")) {
      dayChanged = true;
      return;
    }
    window.location.reload();
  }

  function flushStale() {
    // A deferred rollover first: there is no point refreshing columns that a
    // reload is about to replace wholesale.
    if (dayChanged) {
      reloadForNewDay();
      return;
    }
    var stale = document.querySelectorAll(".column[data-stale]");
    for (var i = 0; i < stale.length; i++) {
      if (!isBusy(stale[i])) refreshColumn(stale[i]);
    }
  }

  function refreshAllColumns() {
    var cols = document.querySelectorAll(".column");
    for (var i = 0; i < cols.length; i++) refreshColumn(cols[i]);
  }

  if (live) {
    var seq = Number(live.dataset.seq || 0);
    var polling = false;

    var poll = function () {
      // A hidden tab has nobody looking at it; it resyncs when it comes back.
      if (polling || document.hidden) return;
      polling = true;

      var url =
        "/changes?board=" + encodeURIComponent(live.dataset.board) + "&since=" + seq;

      fetch(url, { cache: "no-store" })
        .then(function (r) {
          return r.ok ? r.json() : null;
        })
        .then(function (data) {
          if (!data) return;
          // Before any column work: a reload supersedes all of it. Both
          // dates must actually be present — treating a missing one as a
          // mismatch would be an unbroken reload loop on a wall display.
          var rendered = live.dataset.today;
          if (data.today && rendered && data.today !== rendered) {
            reloadForNewDay();
            return;
          }
          // The counter only ever climbs, so a smaller one means the server
          // restarted and its numbering began again. We cannot tell what was
          // missed, so re-fetch the lot.
          if (data.seq < seq) {
            seq = data.seq;
            refreshAllColumns();
            return;
          }
          seq = data.seq;
          for (var i = 0; i < data.keys.length; i++) {
            var col = document.getElementById("col-" + data.keys[i]);
            // A column that is not on screen (another week) needs nothing.
            if (col) refreshColumn(col);
          }
        })
        .catch(function () {
          // Server restarting or the network blinked; the next tick retries.
        })
        .then(function () {
          polling = false;
        });
    };

    setInterval(poll, POLL_MS);
    // Coming back to a tab should feel immediate rather than waiting a tick.
    document.addEventListener("visibilitychange", function () {
      if (!document.hidden) poll();
    });
  }

  // Closing an editor is the usual moment a deferred column can catch up, and
  // the swap handler above covers that. This is the safety net: a column must
  // never sit stale indefinitely because one event did not fire.
  setInterval(flushStale, 5000);

  body.addEventListener("htmx:afterRequest", function (evt) {
    submittedKey = null;

    // A move empties the column the task left, so refresh that one too.
    var xhr = evt.detail && evt.detail.xhr;
    if (!xhr) return;
    var other = xhr.getResponseHeader("X-Refresh-Column");
    if (!other) return;
    var col = document.getElementById("col-" + other);
    if (col && window.htmx) {
      window.htmx.trigger(col, "refresh-column");
    }
  });
})();
