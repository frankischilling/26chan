// Browser services receive their own database connection through the config.
// Clear inherited credentials that the intake runtime deliberately rejects.
export function serviceCredentialOverrides(parent, allowedDatabaseKeys) {
  const allowed = new Set(allowedDatabaseKeys);
  return Object.fromEntries(Object.keys(parent).filter(key => {
    const name = key.toUpperCase();
    return (name.endsWith('DATABASE_URL') && !allowed.has(name))
      || ['AWS_', 'AZURE_', 'GOOGLE_', 'MEDIA_DISPATCH_', 'DEPLOY_'].some(prefix => name.startsWith(prefix))
      || ['GH_TOKEN', 'GITHUB_TOKEN', 'PUBLIC_INTAKE_TOKEN', 'DOCKER_HOST', 'SSH_AUTH_SOCK'].includes(name);
  }).map(key => [key, '']));
}
