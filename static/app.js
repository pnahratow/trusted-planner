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
