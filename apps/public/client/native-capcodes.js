// Finite staff badge recipes from the pinned public API and released client.
import { postFileAssetUrl } from './native-file-presentation.js';
const definitions = [
  ['Mod', 'capcodeMod', 'id_mod', 'Highlight posts by Moderators', 'modicon', 'This user is a board Moderator.'],
  ['Admin', 'capcodeAdmin', 'id_admin', 'Highlight posts by Administrators', 'adminicon', 'This user is a board Administrator.'],
  ['Manager', 'capcodeManager', 'id_manager', 'Highlight posts by Managers', 'managericon', 'This user is a board Manager.'],
  ['Developer', 'capcodeDeveloper', 'id_developer', 'Highlight posts by Developers', 'developericon', 'This user is a board Developer.'],
  ['Founder', 'capcodeAdmin', 'id_admin', 'Highlight posts by the Founder', 'foundericon', "This user is the board's Founder."],
].map(([label, nameClass, group, title, icon, iconTitle]) => Object.freeze({ label, nameClass, group, title, icon, iconTitle }));
const tokens = new Set(['nameBlock', 'capcode', 'identityIcon', 'highlightPost',
  ...definitions.flatMap(value => [value.nameClass, value.group])]);
const icons = new Set(definitions.flatMap(value => [`/static/identity/${value.icon}.gif`,
  ...(value.icon === 'foundericon' ? [] : [`/static/identity/${value.icon}@2x.gif`])]));
export const isCapcodeToken = token => tokens.has(token);
export const postIdentityUrl = value => icons.has(value);
const classes = node => typeof node === 'string' ? [] : (node.attrs.class || '').split(' ');
const exact = (attrs, expected) => Object.keys(attrs).sort().join(',') === Object.keys(expected).sort().join(',')
  && Object.entries(expected).every(([key, value]) => attrs[key] === value);
function require(value) { if (!value) throw new TypeError('invalid-snapshot'); }

// Icons and titles are meaningful only as one complete badge in this post's
// own header. A same-origin URL alone does not grant an image fetch.
export function validateCapcodeTree(tree, no) {
  const marked = [], all = [];
  function collect(node) {
    if (typeof node === 'string') return;
    all.push(node);
    if (classes(node).some(isCapcodeToken) || node.tag === 'strong'
      || (Object.hasOwn(node.attrs, 'srcset') && !postFileAssetUrl(node.attrs.src)) || (node.tag === 'img' && Object.hasOwn(node.attrs, 'title'))
      || postIdentityUrl(node.attrs.src)) marked.push(node);
    node.children.forEach(collect);
  }
  collect(tree);
  const blocks = all.filter(node => classes(node).includes('nameBlock'));
  const mobile = all.find(node => node.tag === 'div' && node.attrs.id === `pim${no}`);
  require(blocks.length <= (mobile ? 2 : 1));
  const permitted = new Set();
  for (const block of blocks) {
    const isMobile = mobile?.children.includes(block);
    if (isMobile && block.attrs.class === 'nameBlock') {
      permitted.add(block); continue; // Full ordinary recipe is checked with the mobile header.
    }
    const header = isMobile ? mobile : all.find(node => node.tag === 'div' && node.attrs.id === `pi${no}`);
    require(header?.children.includes(block) && block.tag === 'span'
      && (isMobile ? block.children.length >= 6 && block.children.length <= 8 : block.children.length === 5));
    const [name, space1, badge, space2, icon] = block.children;
    require(typeof name === 'object' && name.tag === 'span' && (isMobile
      ? name.attrs.class === 'name' && Object.keys(name.attrs).every(key => ['class', 'title'].includes(key))
      : exact(name.attrs, { class: 'name' }))
      && name.children.every(child => typeof child === 'string')
      && new TextEncoder().encode(name.children.join('')).length <= 100
      && space1 === ' ' && space2 === ' '
      && typeof badge === 'object' && badge.tag === 'strong' && badge.children.length === 1
      && typeof icon === 'object' && icon.tag === 'img' && icon.children.length === 0);
    const def = definitions.find(value => badge.children[0] === `## ${value.label}`);
    require(def && exact(block.attrs, { class: `nameBlock ${def.nameClass}` })
      && exact(badge.attrs, { class: `capcode hand ${def.group}`, title: def.title })
      && exact(icon.attrs, { class: 'identityIcon', src: `/static/identity/${def.icon}.gif`,
        ...(def.icon === 'foundericon' ? {} : { srcset: `/static/identity/${def.icon}@2x.gif 2x` }),
        alt: def.iconTitle, title: def.iconTitle, width: '16', height: '16' }));
    for (const node of [block, badge, icon]) permitted.add(node);
    require(!all.some(node => classes(node).some(value => ['posteruid', 'postertrip', 'flag', 'bfl'].includes(value))));
    const highlighted = all.filter(node => classes(node).includes('highlightPost'));
    require(highlighted.length <= 1);
    if (highlighted.length) {
      const post = highlighted[0];
      require(def.label === 'Admin' && post.tag === 'div' && post.attrs.id === `p${no}`
        && classes(post).includes('post'));
      permitted.add(post);
    }
  }
  require(marked.every(node => permitted.has(node)));
}
