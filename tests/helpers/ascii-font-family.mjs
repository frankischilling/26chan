// CSS Fonts matches family names case-insensitively. The pinned comparison
// uses ASCII names; preserve punctuation, order, whitespace and non-ASCII text.
// https://www.w3.org/TR/css-fonts-3/#font-family-casing
export function asciiFontFamily(value) {
  return value.replace(/[A-Z]/g, letter => letter.toLowerCase());
}
