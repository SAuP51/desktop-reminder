param(
    [Parameter(Mandatory=$true)]
    [string]$AgentPath
)

$ErrorActionPreference = "Stop"
$taskName = "ReminderAgent"
$action = New-ScheduledTaskAction -Execute $AgentPath
$trigger = New-ScheduledTaskTrigger -AtLogOn
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -Description "Start Reminder Agent at user logon" -Force | Out-Null
Write-Host "Installed scheduled task: $taskName"
