# Cross-platform deterministic controls; no Windows APIs, compiler or native probe.
# Extract only the production functions, never execute the hosted entry point.
$ErrorActionPreference='Stop'
$tokens=$null; $parseErrors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'run-hosted.ps1'),[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'Hosted launcher parse failed.' }
foreach ($name in @('Save-EnvironmentObservation','Wait-ToolchainEnvironment')) {
  $matches=@($ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name },$true))
  if ($matches.Count -ne 1) { throw 'Production evidence function missing or duplicated.' }
  . ([scriptblock]::Create($matches[0].Extent.Text))
}
function Assert-Control($Condition,[string]$Label) { if (-not $Condition) { throw ('Evidence control failed: ' + $Label) } }
# CLR getters match the production object's exception behavior; no script-property
# adapter can swallow a simulated getter exception before the observer sees it.
Add-Type -TypeDefinition @'
public sealed class ToolchainEvidenceControlChild {
  public bool Finished, RootExited, WaitThrows, ObservationThrows;
  public int Code, WaitBudget, WaitCalls, ExitReads;
  public uint Active;
  public bool WaitForExit(int milliseconds) {
    WaitBudget = milliseconds; WaitCalls++;
    if (WaitThrows) throw new System.InvalidOperationException("primary-wait-control");
    return Finished;
  }
  public bool HasExited { get {
    if (ObservationThrows) throw new System.InvalidOperationException("private-observation-control");
    return RootExited;
  } }
  public int ExitCode { get { ExitReads++; return Code; } }
  public uint ActiveProcessCount { get {
    if (ObservationThrows) throw new System.InvalidOperationException("private-observation-control");
    return Active;
  } }
}
'@
function New-ControlChild([bool]$Finished,[bool]$RootExited,[int]$Code,[uint32]$Active,[bool]$WaitThrows=$false,[bool]$ObservationThrows=$false) {
  $child=[ToolchainEvidenceControlChild]::new()
  $child.Finished=$Finished; $child.RootExited=$RootExited; $child.Code=$Code; $child.Active=$Active
  $child.WaitThrows=$WaitThrows; $child.ObservationThrows=$ObservationThrows
  return $child
}
function Invoke-Control($Child) {
  $script:status=[ordered]@{ environment_evidence=[ordered]@{ checkpoint='launch'; wait_result='not-run'; root_exited=$null; root_exit=$null; active_processes=$null; observation_failed=$false }; failure_class='toolchain-environment-error'; qualified=$false; forced_cleanup=$true }
  $caught=$null
  try { Wait-ToolchainEnvironment -Child $Child } catch { $caught=$_.Exception.Message }
  Assert-Control ($Child.WaitBudget -eq 20000 -and $Child.WaitCalls -eq 1) 'original single wait budget'
  Assert-Control ($status.failure_class -eq 'toolchain-environment-error' -and -not $status.qualified -and $status.forced_cleanup) 'observation cannot change classification or qualification'
  $json=$status.environment_evidence | ConvertTo-Json -Compress
  Assert-Control ($json -notmatch 'private-observation-control|primary-wait-control') 'exception text excluded'
  return $caught
}
$child=New-ControlChild $false $false 0 1
$errorText=Invoke-Control $child
Assert-Control ($errorText -eq 'MSVC environment failed.' -and $status.environment_evidence.wait_result -eq 'deadline' -and $status.environment_evidence.root_exited -eq $false -and $null -eq $status.environment_evidence.root_exit -and $status.environment_evidence.active_processes -eq 1 -and $child.ExitReads -eq 0) 'running root deadline'
$child=New-ControlChild $false $true 0 1
$errorText=Invoke-Control $child
Assert-Control ($errorText -eq 'MSVC environment failed.' -and $status.environment_evidence.wait_result -eq 'deadline' -and $status.environment_evidence.root_exited -eq $true -and $status.environment_evidence.root_exit -eq 0 -and $status.environment_evidence.active_processes -eq 1) 'successful root with surviving child still fails'
$child=New-ControlChild $true $true 7 0
$errorText=Invoke-Control $child
Assert-Control ($errorText -eq 'MSVC environment failed.' -and $status.environment_evidence.wait_result -eq 'completed' -and $status.environment_evidence.checkpoint -eq 'exit-check' -and $status.environment_evidence.root_exit -eq 7 -and $status.environment_evidence.active_processes -eq 0) 'nonzero root still fails'
$child=New-ControlChild $false $false 0 1 $true $true
$errorText=Invoke-Control $child
Assert-Control ($errorText -match 'primary-wait-control' -and $errorText -notmatch 'private-observation-control' -and $status.environment_evidence.wait_result -eq 'exception' -and $status.environment_evidence.observation_failed -and $null -eq $status.environment_evidence.root_exited -and $null -eq $status.environment_evidence.root_exit -and $null -eq $status.environment_evidence.active_processes) 'failed observation preserves primary exception'
$child=New-ControlChild $false $false 0 1 $false $true
$errorText=Invoke-Control $child
Assert-Control ($errorText -eq 'MSVC environment failed.' -and $status.environment_evidence.wait_result -eq 'deadline' -and $status.environment_evidence.observation_failed) 'failed observation preserves deadline'
$child=New-ControlChild $true $true 0 0
$errorText=Invoke-Control $child
Assert-Control ($null -eq $errorText -and $status.environment_evidence.wait_result -eq 'completed' -and $status.environment_evidence.root_exit -eq 0 -and $status.environment_evidence.active_processes -eq 0 -and -not $status.environment_evidence.observation_failed) 'successful wait remains successful'
$child=New-ControlChild $true $true 0 0 $false $true
$errorText=Invoke-Control $child
Assert-Control ($null -eq $errorText -and $status.environment_evidence.wait_result -eq 'completed' -and $status.environment_evidence.observation_failed -and $null -eq $status.environment_evidence.root_exited -and $null -eq $status.environment_evidence.root_exit -and $null -eq $status.environment_evidence.active_processes) 'failed observation preserves successful primary result'
Write-Output 'Toolchain evidence controls passed (7).'
