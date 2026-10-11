$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
try {
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    $null = [Windows.Devices.Geolocation.Geolocator, Windows.Devices.Geolocation, ContentType = WindowsRuntime]
    $null = [Windows.Devices.Geolocation.Geoposition, Windows.Devices.Geolocation, ContentType = WindowsRuntime]
    $asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.IsGenericMethod -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
    } | Select-Object -First 1
    $locator = [Windows.Devices.Geolocation.Geolocator]::new()
    $locator.DesiredAccuracyInMeters = 50
    $operation = $locator.GetGeopositionAsync([TimeSpan]::FromSeconds(15), [TimeSpan]::FromSeconds(50))
    $task = $asTask.MakeGenericMethod([Windows.Devices.Geolocation.Geoposition]).Invoke($null, @($operation))
    if (-not $task.Wait(55000)) { $operation.Cancel(); throw 'Location timed out' }
    $coordinate = $task.Result.Coordinate
    @{ latitude = $coordinate.Point.Position.Latitude; longitude = $coordinate.Point.Position.Longitude;
       accuracy = $coordinate.Accuracy; city = $null } | ConvertTo-Json -Compress
} catch {
    [Console]::Error.WriteLine('Windows location unavailable. Enable Settings > Privacy & security > Location, including desktop app access. ' + $_.Exception.Message)
    exit 1
}
