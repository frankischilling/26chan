import path from 'node:path';
import { existsSync, lstatSync, realpathSync, readdirSync, rmSync } from 'node:fs';

export default async function teardown(config) {
  const root = config.metadata.ownedStaffIntakeRoot;
  const privateRoot = path.resolve('.local');
  if (typeof root !== 'string' || path.dirname(root) !== privateRoot
      || !/^staff-browser-intake-[a-f0-9]{16}$/.test(path.basename(root))) throw new Error('Invalid owned staff intake root');
  if (!existsSync(root)) return;
  if (lstatSync(root).isSymbolicLink() || realpathSync(root) !== path.join(realpathSync(privateRoot), path.basename(root))) throw new Error('Invalid owned staff intake path');
  // This qualification posts no uploads. Refuse cleanup if any input appeared.
  if (readdirSync(root).length !== 0) throw new Error('Owned intake directory is not empty');
  rmSync(root, { recursive: true });
}
