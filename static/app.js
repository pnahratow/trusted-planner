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

  // -------------------------------------------------- click away to cancel
  //
  // An open editor is dismissed by clicking anything that is not part of it,
  // or by pressing Escape. Cancelling swaps the plain row back over just that
  // row, so a second editor elsewhere in the same column is left alone.
  function cancelEditor(editor) {
    var id = editor.dataset.taskId;
    if (!id || editor.dataset.cancelling) return;
    editor.dataset.cancelling = "1"; // a stray second click must not re-fire
    window.htmx.ajax("GET", "/task/" + id + "/row", {
      target: "#task-" + id,
      swap: "outerHTML",
    });
  }

  function cancelEditorsOutside(target) {
    var open = document.querySelectorAll(".task-editing");
    for (var i = 0; i < open.length; i++) {
      if (!open[i].contains(target)) cancelEditor(open[i]);
    }
  }

  document.addEventListener("click", function (evt) {
    // Opening another editor is handled by that row's own request; cancelling
    // this one alongside it is exactly the intent.
    cancelEditorsOutside(evt.target);
  });

  document.addEventListener("keydown", function (evt) {
    if (evt.key !== "Escape") return;
    var editor = evt.target.closest ? evt.target.closest(".task-editing") : null;
    if (editor) cancelEditor(editor);
    else cancelEditorsOutside(document.body);
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
