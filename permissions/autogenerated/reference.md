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
- `allow-available-ports-direct`
- `allow-cancel-read`
- `allow-close`
- `allow-close-all`
- `allow-force-close`
- `allow-open`
- `allow-read`
- `allow-write`
- `allow-write-binary`
- `allow-start-listening`
- `allow-stop-listening`

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

`serialplugin:allow-available-ports-direct`

</td>
<td>

Enables the available_ports_direct command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-available-ports-direct`

</td>
<td>

Denies the available_ports_direct command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-bytes-to-read`

</td>
<td>

Enables the bytes_to_read command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-bytes-to-read`

</td>
<td>

Denies the bytes_to_read command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-bytes-to-write`

</td>
<td>

Enables the bytes_to_write command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-bytes-to-write`

</td>
<td>

Denies the bytes_to_write command without any pre-configured scope.

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

`serialplugin:allow-clear-break`

</td>
<td>

Enables the clear_break command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-clear-break`

</td>
<td>

Denies the clear_break command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-clear-buffer`

</td>
<td>

Enables the clear_buffer command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-clear-buffer`

</td>
<td>

Denies the clear_buffer command without any pre-configured scope.

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

`serialplugin:allow-read-carrier-detect`

</td>
<td>

Enables the read_carrier_detect command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-carrier-detect`

</td>
<td>

Denies the read_carrier_detect command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read-cd`

</td>
<td>

Enables the read_cd command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-cd`

</td>
<td>

Denies the read_cd command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read-clear-to-send`

</td>
<td>

Enables the read_clear_to_send command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-clear-to-send`

</td>
<td>

Denies the read_clear_to_send command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read-cts`

</td>
<td>

Enables the read_cts command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-cts`

</td>
<td>

Denies the read_cts command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read-data-set-ready`

</td>
<td>

Enables the read_data_set_ready command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-data-set-ready`

</td>
<td>

Denies the read_data_set_ready command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read-dsr`

</td>
<td>

Enables the read_dsr command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-dsr`

</td>
<td>

Denies the read_dsr command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read-dtr`

</td>
<td>

Enables the read_dtr command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-dtr`

</td>
<td>

Denies the read_dtr command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read-ri`

</td>
<td>

Enables the read_ri command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-ri`

</td>
<td>

Denies the read_ri command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-read-ring-indicator`

</td>
<td>

Enables the read_ring_indicator command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-read-ring-indicator`

</td>
<td>

Denies the read_ring_indicator command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-set-baud-rate`

</td>
<td>

Enables the set_baud_rate command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-set-baud-rate`

</td>
<td>

Denies the set_baud_rate command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-set-break`

</td>
<td>

Enables the set_break command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-set-break`

</td>
<td>

Denies the set_break command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-set-data-bits`

</td>
<td>

Enables the set_data_bits command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-set-data-bits`

</td>
<td>

Denies the set_data_bits command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-set-flow-control`

</td>
<td>

Enables the set_flow_control command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-set-flow-control`

</td>
<td>

Denies the set_flow_control command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-set-parity`

</td>
<td>

Enables the set_parity command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-set-parity`

</td>
<td>

Denies the set_parity command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-set-stop-bits`

</td>
<td>

Enables the set_stop_bits command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-set-stop-bits`

</td>
<td>

Denies the set_stop_bits command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-set-timeout`

</td>
<td>

Enables the set_timeout command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-set-timeout`

</td>
<td>

Denies the set_timeout command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-start-listening`

</td>
<td>

Enables the start_listening command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-start-listening`

</td>
<td>

Denies the start_listening command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-stop-listening`

</td>
<td>

Enables the stop_listening command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-stop-listening`

</td>
<td>

Denies the stop_listening command without any pre-configured scope.

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

<tr>
<td>

`serialplugin:allow-write-data-terminal-ready`

</td>
<td>

Enables the write_data_terminal_ready command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-write-data-terminal-ready`

</td>
<td>

Denies the write_data_terminal_ready command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-write-dtr`

</td>
<td>

Enables the write_dtr command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-write-dtr`

</td>
<td>

Denies the write_dtr command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-write-request-to-send`

</td>
<td>

Enables the write_request_to_send command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-write-request-to-send`

</td>
<td>

Denies the write_request_to_send command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:allow-write-rts`

</td>
<td>

Enables the write_rts command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`serialplugin:deny-write-rts`

</td>
<td>

Denies the write_rts command without any pre-configured scope.

</td>
</tr>
</table>
