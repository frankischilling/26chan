// Static help stays separate from the editor's draft and popup lifecycle.
// Use text nodes even for examples containing HTML-like or regex characters.
export function appendFilterHelp(detail) {
  const node = (tag, text) => {
    const element = document.createElement(tag);
    element.textContent = text;
    return element;
  };
  const heading = (level, text) => detail.append(node(`h${level}`, text));
  const paragraph = text => detail.append(node('p', text));
  const example = (pattern, explanation) => {
    const line = node('p', '');
    line.append(node('code', pattern), document.createTextNode(`: ${explanation}`));
    detail.append(line);
  };
  const title = node('h2', 'Filters & Highlights Help');
  title.tabIndex = -1;
  title.autofocus = true;
  detail.append(title);

  heading(3, 'Tripcode, Name and ID filters');
  paragraph('These match exact, case-sensitive text. Enter the value as it appears on the post, including the exclamation mark for a tripcode.');
  example('!Ep8pui8Vw2', 'an exact tripcode, including !.');

  heading(3, 'Comment, Subject and Filename filters');
  paragraph('These types accept the patterns below. On pages, Subject filters apply on board indexes, not inside threads.');
  heading(4, 'Matching whole words');
  example('feel', 'matches "feel" or "FEEL", but not "feeling". Whole-word matching is case-insensitive.');
  heading(4, 'AND operator');
  example('feel girlfriend', 'matches both words on the same line, in either order.');
  heading(4, 'Quoted strings');
  example('"that feel when"', 'matches this case-sensitive substring anywhere in the text, without requiring whole-word boundaries.');
  paragraph('The | character still means alternation, even inside quotes: "cat|dog" matches either cat or dog.');
  heading(4, 'Wildcards');
  example('feel*', 'matches "feel", "feels", "feeling" and "feeler".');
  example('idolm*ster', 'matches "idolmaster" or "idolm@ster".');
  paragraph('In a whole-word pattern, * matches zero or more characters, but cannot span whitespace.');
  heading(4, 'Regular expressions');
  paragraph('Use /.../ for a case-sensitive regular expression, or /.../i to ignore case. Only these two forms are recognized; other flags are not supported.');
  example('/feel when no (girl|boy)friend/i', 'matches "feel when no girlfriend" or "feel when no boyfriend", ignoring case.');
  example('/^(?!.*touhou).*$/i', 'a NOT example for single-line text: matches text with no "touhou", ignoring case. This is not a general multiline exclusion.');
  example('/^>/', 'matches comments starting with > (a quote).');
  example('/^$/', 'matches empty or missing comment text on pages. Auto-watching skips absent or empty raw catalog comments, so it cannot discover those threads with this pattern.');

  heading(3, 'Colors');
  paragraph('Choose a swatch or enter a custom color. Supported examples include:');
  const colors = node('p', '');
  ['red', '#0f0', '#00ff00', 'rgba(34, 12, 64, 0.3)'].forEach((color, index) => {
    if (index) colors.append(document.createTextNode(', '));
    colors.append(node('code', color));
  });
  detail.append(colors);
  paragraph('Invalid colors and CSS expressions such as var(...) are rejected. Use Clear to remove a color.');

  heading(3, 'Boards');
  example('a jp', 'lowercase board slugs separated by spaces, without slashes or leading separators.');
  paragraph('Leave Boards blank to apply a filter to posts on all boards. Auto-watching requires explicit boards.');
  heading(3, 'Auto-watching');
  paragraph('Enable filtering and Thread Watcher in Settings, then enable Auto on an active filter and fill in Boards. Manually refreshing the watcher searches catalog JSON for those boards and adds matching threads.');
  heading(3, 'Shortcut');
  const shortcut = node('p', 'With Keyboard shortcuts and filtering enabled, select text and press ');
  shortcut.append(node('kbd', 'F'), document.createTextNode(' to add it to the filter editor. Review and save the new filter.'));
  detail.append(shortcut);
  heading(3, 'Applying filters');
  paragraph('The first matching active filter wins. Hide hides matching content; otherwise the filter highlights it. Hidden content normally has a View control, unless thread stubs are disabled.');
  paragraph('Save stores your filters. Enable "Filter and highlight specific threads/posts" separately in Settings and save those settings to apply them to pages.');
  paragraph('Patterns run in bounded workers. If matching fails or exceeds its limits, content stays visible.');
}
