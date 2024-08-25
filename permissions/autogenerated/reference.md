## Default Permission

# Tauri `serialport` default permissions

This configuration file defines the default permissions granted
to the serialport.

### Granted Permissions

This default permission set enables all read-related commands and
allows access to the `$APP` folder and sub directories created in it.
The location of the `$APP` folder depends on the operating system,
where the application is run.

In general the `$APP` folder needs to be manually created
by the application at runtime, before accessing files or folders
in it is possible.

### Denied Permissions

This default permission set prevents access to critical components
of the Tauri application by default.
On Windows the webview data folder access is denied.



- `allow-available-ports`
- `allow-cancel-read`
- `allow-close`
- `allow-close-all`
- `allow-force-close`
- `allow-open`
- `allow-read`
- `allow-write`
- `allow-write-binary`

## Permission Table 

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`serialplugin:allow-available-ports`

</td>
<td>

Enables the available_ports command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-available-ports`

</td>
<td>

Denies the available_ports command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-cancel-read`

</td>
<td>

Enables the cancel_read command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-cancel-read`

</td>
<td>

Denies the cancel_read command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-close`

</td>
<td>

Enables the close command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-close`

</td>
<td>

Denies the close command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-close-all`

</td>
<td>

Enables the close_all command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-close-all`

</td>
<td>

Denies the close_all command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-force-close`

</td>
<td>

Enables the force_close command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-force-close`

</td>
<td>

Denies the force_close command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-open`

</td>
<td>

Enables the open command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-open`

</td>
<td>

Denies the open command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read`

</td>
<td>

Enables the read command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read`

</td>
<td>

Denies the read command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-write`

</td>
<td>

Enables the write command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-write`

</td>
<td>

Denies the write command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-write-binary`

</td>
<td>

Enables the write_binary command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-write-binary`

</td>
<td>

Denies the write_binary command without any pre-configured scope.

</td>
</tr>
</table>
