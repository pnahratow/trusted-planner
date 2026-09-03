// Progressive enhancement only. Drag & drop (phase 5) and the SSE focus guard
// (phase 6) land here; for now this just keeps typing from being interrupted
// when a column re-renders under the cursor.

(function () {
  "use strict";

  // Swapping a whole column replaces the input you were typing in. Remember
  // which column had focus and where the caret was, and put it back after.
  var pending = null;

  document.body.addEventListener("htmx:beforeSwap", function () {
    var el = document.activeElement;
    if (el && el.classList && el.classList.contains("add-input")) {
      pending = { key: el.dataset.column, value: el.value, caret: el.selectionStart };
    } else {
      pending = null;
    }
  });

  document.body.addEventListener("htmx:afterSwap", function () {
    if (!pending) return;
    var next = document.querySelector('.add-input[data-column="' + pending.key + '"]');
    if (next) {
      next.focus();
      // A submitted add clears the field; anything still typed is restored.
      if (pending.value && next.value === "") {
        next.value = pending.value;
      }
      var at = Math.min(pending.caret || 0, next.value.length);
      next.setSelectionRange(at, at);
    }
    pending = null;
  });

  // A move empties the column the task left, so refresh that one too.
  document.body.addEventListener("htmx:afterRequest", function (evt) {
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
