// Finite public badge recipes from the pinned public API and released client.
import { postFileAssetUrl } from './native-file-presentation.js';
const definitions = [
  ['Mod', 'Mod', 'mod', 'Moderators', 'modicon', 'This user is a board Moderator.'],
  ['Admin', 'Admin', 'admin', 'Administrators', 'adminicon', 'This user is a board Administrator.'],
  ['Manager', 'Manager', 'manager', 'Managers', 'managericon', 'This user is a board Manager.'],
  ['Developer', 'Developer', 'developer', 'Developers', 'developericon', 'This user is a board Developer.'],
  ['Founder', 'Admin', 'admin', 'the Founder', 'foundericon', "This user is the board's Founder."],
  ['Verified', 'Verified', 'verified', 'Verified Users'],
].map(([label, nameClass, group, title, icon, iconTitle]) => Object.freeze({ label, nameClass: `capcode${nameClass}`, group: `id_${group}`, title: `Highlight posts by ${title}`, icon, iconTitle }));
const tokens = new Set(['nameBlock', 'capcode', 'identityIcon', 'highlightPost',
  ...definitions.flatMap(value => [value.nameClass, value.group])]);
const icons = new Set(definitions.flatMap(value => value.icon ? [`/static/identity/${value.icon}.gif`,
  ...(value.icon === 'foundericon' ? [] : [`/static/identity/${value.icon}@2x.gif`])] : []));
export const isCapcodeToken = token => tokens.has(token);
export const postIdentityUrl = value => icons.has(value);
const classes = node => typeof node === 'string' ? [] : (node.attrs.class || '').split(' ');
import { exactAttributes as exact } from './native-attribute-keys.js';
function require(value) { if (!value) throw new TypeError('invalid-snapshot'); }
const encoder = new TextEncoder();
const escapedName = value => value.replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' })[char]);

// Icons and titles are meaningful only as one complete badge in this post's
// own header. A same-origin URL alone does not grant an image fetch.
export function validateCapcodeTree(tree, no) {
  const marked = [], all = [];
  function collect(node, comment = false) {
    if (typeof node === 'string') return;
    all.push(node);
    const notice = node.tag === 'strong' && node.attrs.class === 'post-notice';
    if (notice) require(comment && exact(node.attrs, { class: 'post-notice' })
      && node.children.every(child => typeof child === 'string')
      && encoder.encode(node.children.join('')).length <= 4098);
    if (classes(node).some(isCapcodeToken) || (node.tag === 'strong' && !notice)
      || (Object.hasOwn(node.attrs, 'srcset') && !postFileAssetUrl(node.attrs.src)) || (node.tag === 'img' && Object.hasOwn(node.attrs, 'title'))
      || postIdentityUrl(node.attrs.src)) marked.push(node);
    const message = node.tag === 'blockquote' && node.attrs.class === 'postMessage' && node.attrs.id === `m${no}`;
    node.children.forEach(child => collect(child, comment || message));
  }
  collect(tree);
  const blocks = all.filter(node => classes(node).includes('nameBlock'));
  const mobile = all.find(node => node.tag === 'div' && node.attrs.id === `pim${no}`);
  require(blocks.length <= (mobile ? 2 : 1));
  const permitted = new Set(), permittedTrips = new Set();
  let badged = false;
  for (const block of blocks) {
    const isMobile = mobile?.children.includes(block);
    if (isMobile && block.attrs.class === 'nameBlock') {
      permitted.add(block); continue; // Full ordinary recipe is checked with the mobile header.
    }
    const header = isMobile ? mobile : all.find(node => node.tag === 'div' && node.attrs.id === `pi${no}`);
    const trip = typeof block.children[2] === 'object' && classes(block.children[2]).includes('postertrip')
      ? block.children[2] : null;
    require(header?.children.includes(block) && block.tag === 'span');
    const [name, space1] = block.children;
    const [badge, space2, icon] = block.children.slice(trip ? 4 : 2);
    if (trip) {
      require(exact(trip.attrs, { class: 'postertrip' }) && trip.tag === 'span'
        && trip.children.length === 1 && typeof trip.children[0] === 'string'
        && /^(?:![./A-Za-z0-9]{10}|!![A-Za-z0-9+/]{11})$/.test(trip.children[0])
        && block.children[3] === ' ');
      permittedTrips.add(trip);
    }
    const verified = block.attrs.class === 'nameBlock capcodeVerified';
    require(typeof name === 'object' && name.tag === 'span' && (isMobile
      ? name.attrs.class === 'name' && Object.keys(name.attrs).every(key => ['class', 'title'].includes(key))
      : exact(name.attrs, { class: 'name' }))
      && name.children.every(child => typeof child === 'string')
      && encoder.encode((verified ? value => value : escapedName)(name.attrs.title ?? name.children.join(''))
        + (trip ? `</span> <span class="postertrip">${trip.children[0]}` : '')).length <= (verified ? 100 : 255)
      && space1 === ' ' && typeof badge === 'object' && badge.tag === 'strong' && badge.children.length === 1);
    const def = definitions.find(value => badge.children[0] === `## ${value.label}`);
    require(def && exact(block.attrs, { class: `nameBlock ${def.nameClass}` })
      && exact(badge.attrs, { class: `capcode hand ${def.group}`, title: def.title }));
    const baseLength = (trip ? 5 : 3) + (def.icon ? 2 : 0);
    require(isMobile ? [baseLength + 1, baseLength + 3].includes(block.children.length) : block.children.length === baseLength);
    if (def.icon) require(space2 === ' ' && typeof icon === 'object' && icon.tag === 'img' && icon.children.length === 0
      && exact(icon.attrs, { class: 'identityIcon', src: `/static/identity/${def.icon}.gif`,
        ...(def.icon === 'foundericon' ? {} : { srcset: `/static/identity/${def.icon}@2x.gif 2x` }),
        alt: def.iconTitle, title: def.iconTitle, width: '16', height: '16' }));
    for (const node of [block, badge]) permitted.add(node);
    if (def.icon) permitted.add(icon);
    badged = true;
    const highlighted = all.filter(node => classes(node).includes('highlightPost'));
    require(highlighted.length <= 1);
    if (highlighted.length) {
      const post = highlighted[0];
      require(def.label === 'Admin' && post.tag === 'div' && post.attrs.id === `p${no}`
        && classes(post).includes('post'));
      permitted.add(post);
    }
  }
  if (badged) require(!all.some(node => classes(node).some(value => ['posteruid', 'flag', 'bfl'].includes(value)))
    && all.filter(node => classes(node).includes('postertrip')).every(node => permittedTrips.has(node)));
  require(marked.every(node => permitted.has(node)));
}
