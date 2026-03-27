// ThistleOS Docs — sortable + searchable tables
// Automatically enhances all tables with class "data-table"
// SPDX-License-Identifier: BSD-3-Clause

(function () {
  'use strict';

  document.addEventListener('DOMContentLoaded', function () {
    document.querySelectorAll('table.data-table').forEach(enhance);
  });

  function enhance(table) {
    // Skip tiny tables (< 3 rows)
    var rows = table.querySelectorAll('tbody tr');
    if (rows.length < 3) return;

    // Add search box
    var wrap = document.createElement('div');
    wrap.className = 'table-search-wrap';
    var input = document.createElement('input');
    input.type = 'text';
    input.className = 'table-search';
    input.placeholder = 'Search\u2026';
    input.setAttribute('aria-label', 'Filter table rows');
    wrap.appendChild(input);
    table.parentNode.insertBefore(wrap, table);

    input.addEventListener('input', function () {
      var q = this.value.toLowerCase();
      rows.forEach(function (row) {
        var text = row.textContent.toLowerCase();
        row.style.display = text.indexOf(q) !== -1 ? '' : 'none';
      });
    });

    // Make headers sortable
    var headers = table.querySelectorAll('thead th');
    headers.forEach(function (th, colIdx) {
      th.style.cursor = 'pointer';
      th.style.userSelect = 'none';
      th.title = 'Click to sort';

      // Add sort indicator
      var arrow = document.createElement('span');
      arrow.className = 'sort-arrow';
      arrow.textContent = ' \u2195';
      arrow.style.opacity = '0.3';
      arrow.style.fontSize = '10px';
      th.appendChild(arrow);

      var ascending = true;
      th.addEventListener('click', function () {
        var rowsArr = Array.from(rows);
        rowsArr.sort(function (a, b) {
          var aText = cellText(a, colIdx);
          var bText = cellText(b, colIdx);

          // Try numeric sort first
          var aNum = parseFloat(aText.replace(/,/g, ''));
          var bNum = parseFloat(bText.replace(/,/g, ''));
          if (!isNaN(aNum) && !isNaN(bNum)) {
            return ascending ? aNum - bNum : bNum - aNum;
          }

          // Fall back to string sort
          return ascending
            ? aText.localeCompare(bText)
            : bText.localeCompare(aText);
        });

        // Re-append in sorted order
        var tbody = table.querySelector('tbody');
        rowsArr.forEach(function (row) {
          tbody.appendChild(row);
        });

        // Update arrows
        headers.forEach(function (h) {
          var a = h.querySelector('.sort-arrow');
          if (a) { a.textContent = ' \u2195'; a.style.opacity = '0.3'; }
        });
        arrow.textContent = ascending ? ' \u2191' : ' \u2193';
        arrow.style.opacity = '0.8';

        ascending = !ascending;
        // Re-grab rows for search
        rows = table.querySelectorAll('tbody tr');
      });
    });
  }

  function cellText(row, idx) {
    var cell = row.children[idx];
    if (!cell) return '';
    return (cell.textContent || '').trim();
  }
})();
