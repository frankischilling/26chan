// Uploaded filenames remain display metadata. PNG describes the approved bytes.
export function fileLabel(filename, op) {
  const dot = filename.lastIndexOf('.'), stem = dot > 0 ? filename.slice(0, dot) : filename;
  const extension = dot > 0 ? filename.slice(dot) : '';
  return stem.length > (op ? 40 : 30) ? (stem.slice(0, op ? 35 : 25) + '(...)' + extension).toWellFormed() : filename;
}

export function validateFilePresentation(tree, context, no) {
  const require = value => { if (!value) throw new TypeError('invalid-file-presentation'); };
  const elements = node => node.children.filter(child => typeof child !== 'string');
  const text = node => node.children.map(child => typeof child === 'string' ? child : text(child)).join('');
  const bounded = value => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= 255
    && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
  function visit(node, parent, grandparent) {
    if (typeof node === 'string') return;
    if ((node.attrs.class || '').split(' ').includes('fileText')) {
      require(node.attrs.class === 'fileText' && node.tag === 'div' && node.attrs.id === `fT${no}` && parent?.attrs.class === 'file'
        && parent.attrs.id === `f${no}` && node.children.length === 3 && node.children[0] === 'File: ');
      const link = node.children[1], tail = node.children[2];
      require(link?.tag === 'a' && elements(node).length === 1 && typeof tail === 'string'
        && /^ \([0-9]+(?:\.[0-9]{1,2})? (?:B|KB|MB), [1-9][0-9]{0,4}x[1-9][0-9]{0,4}\)$/.test(tail)
        && link.children.every(child => typeof child === 'string'));
      const prefix = `${context.mediaOrigin}/${context.board}/`;
      require(context.mediaOrigin && link.attrs.href?.startsWith(prefix)
        && /^[1-9][0-9]{0,18}\.png$/.test(link.attrs.href.slice(prefix.length)));
      const thumb = elements(parent).find(child => child.attrs.class === 'fileThumb');
      if (thumb) {
        require(elements(thumb).length === 2 && elements(thumb).filter(child => child.attrs.class === 'mFileInfo mobile').length === 1);
        const image = elements(thumb).find(child => child.tag === 'img');
        require(image && bounded(image.attrs.alt) && link.attrs.href === thumb.attrs.href
          && [link.attrs.href, link.attrs.href.slice(0, -4) + 's.jpg'].includes(image.attrs.src)
          && text(link) === fileLabel(image.attrs.alt, no === context.thread));
        require(['width', 'height'].every(key => /^[1-9][0-9]{0,2}$/.test(image.attrs[key] ?? '') && Number(image.attrs[key]) <= 250));
        require(link.attrs.title === undefined ? text(link) === image.attrs.alt : link.attrs.title === image.attrs.alt);
      } else require(text(link) === 'Spoiler Image' && bounded(link.attrs.title));
    }
    if ((node.attrs.class || '').split(' ').includes('mFileInfo')) {
      require(node.tag === 'div' && node.attrs.class === 'mFileInfo mobile' && node.children.length === 1
        && typeof node.children[0] === 'string' && parent?.tag === 'a' && parent.attrs.class === 'fileThumb'
        && grandparent?.attrs.class === 'file' && grandparent.attrs.id === `f${no}`);
      const header = elements(grandparent).find(child => child.attrs.class === 'fileText');
      require(header && parent.attrs.href === header.children[1]?.attrs.href);
      const size = /^ \(([^,]+),/.exec(header.children[2]);
      require(size && node.children[0] === `${size[1]} PNG`);
    }
    for (const child of node.children) visit(child, node, parent);
  }
  visit(tree, null, null);
}
