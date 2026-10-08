'use strict';
const board = document.getElementById('staff-board');
const comment = document.getElementById('staff-comment');
const flag = document.getElementById('staff-flag');
const choices = document.getElementById('staff-flag-choices')?.content;
if (board && comment) {
  const update = () => {
    const maximum = Number(board.selectedOptions[0]?.dataset.commentLimit);
    if (Number.isSafeInteger(maximum) && maximum >= 1 && maximum <= 50000) {
      comment.maxLength = maximum * 2;
    }
    if (flag && choices) {
      const policy = board.selectedOptions[0]?.dataset;
      const allowed = new Set((policy?.flags || '').split(' '));
      const prior = flag.value;
      const none = document.createElement('option');
      none.value = ''; none.textContent = 'None';
      flag.replaceChildren(none);
      for (const option of choices.querySelectorAll('option')) {
        if (option.dataset.flagType === policy?.flagType && allowed.has(option.value)) {
          flag.append(option.cloneNode(true));
        }
      }
      if ([...flag.options].some(option => option.value === prior)) flag.value = prior;
    }
  };
  board.addEventListener('change', update);
  update();
}
