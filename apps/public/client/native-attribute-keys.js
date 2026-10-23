export const attributeKeys = attrs => Object.keys(attrs).sort().join(',');

export const exactAttributes = (attrs, expected) => attributeKeys(attrs) === attributeKeys(expected)
  && Object.entries(expected).every(([key, value]) => attrs[key] === value);
