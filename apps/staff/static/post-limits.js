'use strict';
const board = document.getElementById('staff-board');
const comment = document.getElementById('staff-comment');
if (board && comment) {
  const update = () => {
    const maximum = Number(board.selectedOptions[0]?.dataset.commentLimit);
    if (Number.isSafeInteger(maximum) && maximum >= 1 && maximum <= 50000) {
      comment.maxLength = maximum * 2;
    }
  };
  board.addEventListener('change', update);
  update();
}
