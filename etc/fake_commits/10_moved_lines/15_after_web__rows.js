function highlightCurrentRow(rows, cursorIndex) {
  rows.forEach((row) => row.classList.remove("current"));
  rows[cursorIndex].classList.add("current");
}

function scrollToCurrentRow(rows, cursorIndex) {
  const currentRow = rows[cursorIndex];
  currentRow.scrollIntoView({ block: "center" });
}
