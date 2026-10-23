import { attributeKeys } from './native-attribute-keys.js';

import { isSpoilerAssetPath } from './native-spoiler-assets.js';

// Uploaded filenames remain display metadata. The approved URL supplies format.
export function postFileAssetUrl(value) {
  return isSpoilerAssetPath(value) || ['/static/catalog/filedeleted-res.gif', '/static/catalog/filedeleted-res@2x.gif'].includes(value);
}

export function fileLabel(filename, op) {
  const dot = filename.lastIndexOf('.'), stem = dot > 0 ? filename.slice(0, dot) : filename;
  const extension = dot > 0 ? filename.slice(dot) : '';
  return stem.length > (op ? 40 : 30) ? (stem.slice(0, op ? 35 : 25) + '(...)' + extension).toWellFormed() : filename;
}

export function validateFilePresentation(tree, context, no) {
  const require = value => { if (!value) throw new TypeError('invalid-file-presentation'); };
  const elements = node => node.children.filter(child => typeof child !== 'string');
  const hasClass = (node, name) => (node.attrs.class || '').split(' ').includes(name);
  const thumbClass = value => ['fileThumb', 'fileThumb imgspoiler'].includes(value);
  const text = node => node.children.map(child => typeof child === 'string' ? child : text(child)).join('');
  const bounded = value => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= 260
    && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
  function visit(node, parent, grandparent) {
    if (typeof node === 'string') return;
    if (node.attrs['data-m'] !== undefined) {
      require(node.tag === 'a' && thumbClass(node.attrs.class)
        && parent?.attrs.class === 'file' && node.attrs['data-m'] === ''
        && /\.(?:png|jpg)$/.test(node.attrs.href));
    }
    if (hasClass(node, 'fileText')) {
      require(node.attrs.class === 'fileText' && node.tag === 'div' && node.attrs.id === `fT${no}` && parent?.attrs.class === 'file'
        && parent.attrs.id === `f${no}` && node.children.length === 3 && node.children[0] === 'File: ');
      const link = node.children[1], tail = node.children[2];
      if (context.board === 'f' && link?.attrs?.href?.endsWith('.swf')) {
        // Source JSON rendering is distinct from the initial upload-board PHP
        // page. A file has one canonical download link and no thumbnail/player.
        const label = link?.tag === 'a' ? text(link) : '';
        const match = typeof tail === 'string' && /^ \(([0-9]+(?:\.[0-9]{1,2})?) (B|KB|MB), ([1-9][0-9]{0,8})x([1-9][0-9]{0,8}), (Hentai|Porn|Japanese|Anime|Game|Loop|Other|null|undefined)\)$/.exec(tail);
        const encoded = encodeURIComponent(label).replace(/[!'()*]/g, value => '%' + value.charCodeAt(0).toString(16).toUpperCase());
        require(no === context.thread && bounded(label) && label.endsWith('.swf') && !label.startsWith('.')
          && elements(node).length === 1 && link.children.every(child => typeof child === 'string')
          && match && Number(match[3]) <= 107374182 && Number(match[4]) <= 107374182
          && Number(match[1]) * ({B:1,KB:1024,MB:1048576}[match[2]]) <= 10485760
          && context.mediaOrigin && link.attrs.href === `${context.mediaOrigin}/f/${encoded}`
          && attributeKeys(link.attrs) === 'href,rel,target' && link.attrs.target === '_blank' && link.attrs.rel === 'noopener noreferrer'
          && attributeKeys(node.attrs) === 'class,id' && attributeKeys(parent.attrs) === 'class,id'
          && link.attrs.title === undefined && node.attrs.title === undefined && elements(parent).length === 1);
        return;
      }
      require(link?.tag === 'a' && elements(node).length === 1 && typeof tail === 'string'
        && /^ \([0-9]+(?:\.[0-9]{1,2})? (?:B|KB|MB), (?:PDF|[1-9][0-9]{0,4}x[1-9][0-9]{0,4})\)$/.test(tail)
        && link.children.every(child => typeof child === 'string'));
      const prefix = `${context.mediaOrigin}/${context.board}/`;
      require(context.mediaOrigin && link.attrs.href?.startsWith(prefix)
        && /^[1-9][0-9]{0,18}\.(?:png|jpg|gif|pdf|webm|mp4)$/.test(link.attrs.href.slice(prefix.length)));
      const pdf = link.attrs.href.endsWith('.pdf');
      require(pdf === tail.endsWith(', PDF)'));
      const thumb = elements(parent).find(child => thumbClass(child.attrs.class));
      if (thumb) {
        require(elements(thumb).length === 2 && elements(thumb).filter(child => child.attrs.class === 'mFileInfo mobile').length === 1);
        const image = elements(thumb).find(child => child.tag === 'img');
        require(image && link.attrs.href === thumb.attrs.href);
        if (thumb.attrs.class === 'fileThumb imgspoiler') {
          require(bounded(node.attrs.title) && text(link) === 'Spoiler Image'
            && (link.attrs.title === undefined || link.attrs.title === node.attrs.title)
            && isSpoilerAssetPath(image.attrs.src) && image.attrs.width === '100' && image.attrs.height === '100'
            && image.attrs.alt === /^ \(([^,]+),/.exec(tail)[1]);
          require(parent.attrs['data-image-spoiler'] === undefined || parent.attrs['data-image-spoiler'] === 'true'
            && parent.attrs['data-image-filename'] === node.attrs.title);
        } else {
          require(node.attrs.title === undefined && bounded(image.attrs.alt)
            && (/\.(?:pdf|webm|mp4)$/.test(link.attrs.href) ? image.attrs.src === link.attrs.href.slice(0, link.attrs.href.lastIndexOf('.')) + 's.jpg'
              : [link.attrs.href, link.attrs.href.slice(0, -4) + 's.jpg'].includes(image.attrs.src))
            && text(link) === fileLabel(image.attrs.alt, no === context.thread));
          require(['width', 'height'].every(key => /^[1-9][0-9]{0,2}$/.test(image.attrs[key] ?? '') && Number(image.attrs[key]) <= 250));
          require(link.attrs.title === undefined ? text(link) === image.attrs.alt : link.attrs.title === image.attrs.alt);
        }
      } else require(text(link) === 'Spoiler Image' && bounded(link.attrs.title));
    }
    if (hasClass(node, 'mFileInfo')) {
      require(node.tag === 'div' && node.attrs.class === 'mFileInfo mobile' && node.children.length === 1
        && typeof node.children[0] === 'string' && parent?.tag === 'a' && thumbClass(parent.attrs.class)
        && grandparent?.attrs.class === 'file' && grandparent.attrs.id === `f${no}`);
      const header = elements(grandparent).find(child => child.attrs.class === 'fileText');
      require(header && parent.attrs.href === header.children[1]?.attrs.href);
      const size = /^ \(([^,]+),/.exec(header.children[2]);
      const extension = /\.(png|jpg|gif|pdf|webm|mp4)$/.exec(parent.attrs.href);
      require(size && extension && node.children[0] === `${size[1]} ${extension[1].toUpperCase()}`);
    }
    if (hasClass(node, 'imgspoiler')) {
      require(node.tag === 'a' && node.attrs.class === 'fileThumb imgspoiler'
        && parent?.attrs.class === 'file' && parent.attrs.id === `f${no}`
        && elements(parent).some(child => child.attrs.class === 'fileText'));
    }
    if (postFileAssetUrl(node.attrs.src) || hasClass(node, 'fileDeletedRes')) {
      if (isSpoilerAssetPath(node.attrs.src)) {
        require(node.tag === 'img' && parent?.attrs.class === 'fileThumb imgspoiler');
      } else {
        require(node.tag === 'img' && node.attrs.class === 'fileDeletedRes'
          && node.attrs.src === '/static/catalog/filedeleted-res.gif'
          && node.attrs.srcset === '/static/catalog/filedeleted-res@2x.gif 2x'
          && node.attrs.alt === 'File deleted.' && node.attrs.width === '127' && node.attrs.height === '13'
          && parent?.tag === 'span' && parent.attrs.class === 'fileThumb' && parent.children.length === 1
          && grandparent?.attrs.class === 'file' && grandparent.attrs.id === `f${no}` && grandparent.children.length === 1);
      }
    }
    for (const child of node.children) visit(child, node, parent);
  }
  visit(tree, null, null);
}
