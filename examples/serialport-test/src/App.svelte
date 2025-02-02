<script lang="ts">
  import { SerialPort } from 'tauri-plugin-serialplugin';
  import { onMount } from 'svelte';
  import {ClearBuffer, DataBits, FlowControl, Parity, StopBits} from "../../../guest-js";

  let serialport: SerialPort | undefined = $state(undefined);
  let name: string = $state('');
  let ports: { [key: string]: { type: string } } = $state({});
  let directPorts: { [key: string]: { type: string } } = $state({});
  let managedPorts: string[] = $state([]);
  let baudRate: number = $state(9600);
  let message: string = $state('');
  let receivedData: string = $state('');
  let isConnected: boolean = $state(false);
  let bytesToRead: number = $state(0);
  let bytesToWrite: number = $state(0);

  // Control signals
  let rtsState: boolean = $state(false);
  let dtrState: boolean = $state(false);
  let ctsState: boolean = $state(false);
  let dsrState: boolean = $state(false);
  let riState: boolean = $state(false);
  let cdState: boolean = $state(false);

  // Port settings
  let selectedDataBits: DataBits = $state(DataBits.Eight);
  let selectedFlowControl: FlowControl = $state(FlowControl.None);
  let selectedParity: Parity = $state(Parity.None);
  let selectedStopBits: StopBits = $state(StopBits.One);
  let timeout: number = $state(1000);

  const dataBitsOptions: DataBits[] = [5, 6, 7, 8];
  const flowControlOptions: FlowControl[] = [FlowControl.None, FlowControl.Software, FlowControl.Hardware];
  const parityOptions: Parity[] = [Parity.None, Parity.Odd, Parity.Even];
  const stopBitsOptions: StopBits[] = [1, 2];

  $inspect(ports).with(console.log);
  $inspect(directPorts).with(console.log);

  async function scanPorts() {
    try {
      ports = await SerialPort.available_ports();
      console.log('Available ports');
    } catch (err) {
      console.error('Failed to scan ports:', err);
    }
  }

  async function scanPortsDirect() {
    try {
      directPorts = await SerialPort.available_ports_direct();
      console.log('Direct ports');
    } catch (err) {
      console.error('Failed to scan ports directly:', err);
    }
  }

  async function showManagedPorts() {
    try {
      managedPorts = await SerialPort.managed_ports();
      console.log('Managed ports');
    } catch (err) {
      console.error('Failed to get Managed ports:', err);
    }
  }

  async function connect() {
    try {
      serialport = new SerialPort({
        path: name,
        baudRate,
        dataBits: selectedDataBits,
        flowControl: selectedFlowControl,
        parity: selectedParity,
        stopBits: selectedStopBits,
        timeout
      });

      await serialport.open();
      isConnected = true;
      console.log('Connected to port:', name);

      await serialport.startListening();


      // Start listening for data
      serialport.listen((data) => {
        console.log('listen', data)
        receivedData += data;
        updatePortStatus();
      });

      // Listen for disconnection
      serialport.disconnected(() => {
        isConnected = false;
        console.log('Port disconnected:', name);
      });
    } catch (err) {
      console.error('Failed to connect:', err);
    }
  }

  async function disconnect() {
    try {
      if (serialport) {
        await serialport.close();
        isConnected = false;
        console.log('Disconnected from port:', name);
      }
    } catch (err) {
      console.error('Failed to disconnect:', err);
    }
  }

  async function sendMessage() {
    try {
      if (serialport && message) {
        await serialport.write(message);
        console.log('Message sent:', message);
        message = '';
        updatePortStatus();
      }
    } catch (err) {
      console.error('Failed to send message:', err);
    }
  }

  async function sendBinary() {
    try {
      if (serialport) {
        const data = new Uint8Array([1, 2, 3, 4, 5]);
        await serialport.writeBinary(data);
        console.log('Binary data sent:', data);
        updatePortStatus();
      }
    } catch (err) {
      console.error('Failed to send binary data:', err);
    }
  }

  async function read() {
      const data = await serialport.read();
      console.log("read:", data);
  }

  async function readBinary() {
      const binaryData = await serialport.readBinary({ size: 3, timeout: 2000 });
      console.log("read binary:", binaryData);
 }

  async function updatePortStatus() {
    if (serialport) {
      try {
        // Update bytes status
        bytesToRead = await serialport.bytesToRead();
        bytesToWrite = await serialport.bytesToWrite();

        // Update control signals
        ctsState = await serialport.readClearToSend();
        dsrState = await serialport.readDataSetReady();
        riState = await serialport.readRingIndicator();
        cdState = await serialport.readCarrierDetect();
      } catch (err) {
        console.error('Failed to update port status:', err);
      }
    }
  }

  async function toggleRTS() {
    try {
      if (serialport) {
        rtsState = !rtsState;
        await serialport.setRequestToSend(rtsState);
      }
    } catch (err) {
      console.error('Failed to toggle RTS:', err);
    }
  }

  async function toggleDTR() {
    try {
      if (serialport) {
        dtrState = !dtrState;
        await serialport.setDataTerminalReady(dtrState);
      }
    } catch (err) {
      console.error('Failed to toggle DTR:', err);
    }
  }

  async function updatePortSettings() {
    if (serialport) {
      try {
        await serialport.setBaudRate(baudRate);
        await serialport.setDataBits(selectedDataBits);
        await serialport.setFlowControl(selectedFlowControl);
        await serialport.setParity(selectedParity);
        await serialport.setStopBits(selectedStopBits);
        await serialport.setTimeout(timeout);
        console.log('Port settings updated');
      } catch (err) {
        console.error('Failed to update port settings:', err);
      }
    }
  }

  async function clearBuffers() {
    try {
      if (serialport) {
        await serialport.clearBuffer(ClearBuffer.All);
        console.log('Buffers cleared');
        updatePortStatus();
      }
    } catch (err) {
      console.error('Failed to clear buffers:', err);
    }
  }

  onMount(() => {
    scanPorts();
  });

  let isManagedPort = $derived(managedPorts.includes(name));
</script>

<main class="container">
  <h1>Tauri Serial Port Demo</h1>

  <!-- Port Scanning -->
  <div class="section">
    <h2>Port Discovery</h2>
    <div class="row">
      <button onclick={scanPorts}>Scan Ports</button>
    </div>

    <!-- Available Ports List -->
    <div class="ports-list">
      <h3>Available Ports</h3>
      {#if Object.keys(ports).length > 0}
        <ul>
          {#each Object.entries(ports) as [portName, info]}
            <li>
              <button
                class="port-select"
                onclick={() => name = portName}
                class:selected={name === portName}
              >
                <strong>{portName}:</strong> {info.type}
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p>No ports found</p>
      {/if}
    </div>

    <div class="row">
      <button onclick={scanPortsDirect}>Scan Ports Direct</button>
    </div>

    <div class="ports-list">
      <h3>Available Direct Ports</h3>
      {#if Object.keys(directPorts).length > 0}
        <ul>
          {#each Object.entries(directPorts) as [portName, info]}
            <li>
              <button
                  class="port-select"
                  onclick={() => name = portName}
                  class:selected={name === portName}
              >
                <strong>{portName}:</strong> {info.type}
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p>No Direct Ports found</p>
      {/if}
    </div>


      <div class="row">
        <button onclick={showManagedPorts}>Show Managed Ports</button>
      </div>

    <!-- Managed Ports List -->
    <div class="ports-list">
      <h3>Managed Ports</h3>
      {#if managedPorts.length > 0}
        <ul>
          {#each managedPorts as portName}
            <li>
              <button
                  class="port-select"
                  onclick={() => name = portName}
                  class:selected={name === portName}
              >
                <strong>{portName}</strong>
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p>No Managed Ports found</p>
      {/if}
    </div>
  </div>

  <!-- Connection Controls -->
  <div class="section">
    <h2>Connection</h2>
    {#if isConnected}
        <h3>Connected to: {serialport.options.path}</h3>
    {:else}
        <p>No Port Connected</p>
    {/if}
    <div class="row">
      <button onclick={connect} disabled={!name || isConnected}>
        Connect to {name}
      </button>
      <button onclick={disconnect} disabled={!isConnected}>
        {#if isConnected}
            Disconnect from {serialport.options.path}
        {:else}
            Disconnect
        {/if}
      </button>
    </div>
  </div>

  <!-- Port Settings -->
  <div class="section">
    <h2>Port Settings</h2>
    <div class="settings-grid">
      <div class="setting">
        <label for="baudRate">Baud Rate:</label>
        <input type="number" id="baudRate" bind:value={baudRate} />
      </div>

      <div class="setting">
        <label for="dataBits">Data Bits:</label>
        <select id="dataBits" bind:value={selectedDataBits}>
          {#each dataBitsOptions as option}
            <option value={option}>{option}</option>
          {/each}
        </select>
      </div>

      <div class="setting">
        <label for="flowControl">Flow Control:</label>
        <select id="flowControl" bind:value={selectedFlowControl}>
          {#each flowControlOptions as option}
            <option value={option}>{option}</option>
          {/each}
        </select>
      </div>

      <div class="setting">
        <label for="parity">Parity:</label>
        <select id="parity" bind:value={selectedParity}>
          {#each parityOptions as option}
            <option value={option}>{option}</option>
          {/each}
        </select>
      </div>

      <div class="setting">
        <label for="stopBits">Stop Bits:</label>
        <select id="stopBits" bind:value={selectedStopBits}>
          {#each stopBitsOptions as option}
            <option value={option}>{option}</option>
          {/each}
        </select>
      </div>

      <div class="setting">
        <label for="timeout">Timeout (ms):</label>
        <input type="number" id="timeout" bind:value={timeout} />
      </div>
    </div>

    <div class="row">
      <button onclick={updatePortSettings} disabled={!isConnected}>
        Update Settings
      </button>
    </div>
  </div>

  <!-- Data Transfer -->
  <div class="section">
    <h2>Data Transfer</h2>
    <div class="row">
      <input
        type="text"
        placeholder="Enter message..."
        bind:value={message}
        disabled={!isConnected}
      />
      <button onclick={sendMessage} disabled={!isConnected || !message}>
        Send Text
      </button>
      <button onclick={sendBinary} disabled={!isConnected}>
        Send Binary
      </button>
      <button onclick={clearBuffers} disabled={!isConnected}>
        Clear Buffers
      </button>

      <button onclick={read} disabled={!isConnected}>
        Read
      </button>
      <button onclick={readBinary} disabled={!isConnected}>
        Read Binary
      </button>
    </div>

    <div class="status-info">
      <p>Bytes to read: {bytesToRead}</p>
      <p>Bytes to write: {bytesToWrite}</p>
    </div>

    <div class="received-data">
      <h3>Received Data:</h3>
      <pre>{receivedData}</pre>
    </div>
  </div>

  <!-- Control Signals -->
  <div class="section">
    <h2>Control Signals</h2>
    <div class="signals-grid">
      <div class="signal">
        <button
          onclick={toggleRTS}
          disabled={!isConnected}
          class:active={rtsState}
        >
          RTS: {rtsState ? 'ON' : 'OFF'}
        </button>
      </div>

      <div class="signal">
        <button
          onclick={toggleDTR}
          disabled={!isConnected}
          class:active={dtrState}
        >
          DTR: {dtrState ? 'ON' : 'OFF'}
        </button>
      </div>

      <div class="signal">
        <div class="indicator" class:active={ctsState}>
          CTS: {ctsState ? 'ON' : 'OFF'}
        </div>
      </div>

      <div class="signal">
        <div class="indicator" class:active={dsrState}>
          DSR: {dsrState ? 'ON' : 'OFF'}
        </div>
      </div>

      <div class="signal">
        <div class="indicator" class:active={riState}>
          RI: {riState ? 'ON' : 'OFF'}
        </div>
      </div>

      <div class="signal">
        <div class="indicator" class:active={cdState}>
          CD: {cdState ? 'ON' : 'OFF'}
        </div>
      </div>
    </div>
  </div>
</main>

<style>
  .container {
    max-width: 1200px;
    margin: 0 auto;
    padding: 20px;
  }

  .section {
    margin-bottom: 30px;
    padding: 20px;
    background-color: #f9f9f9;
    border-radius: 8px;
    box-shadow: 0 2px 4px rgba(0,0,0,0.1);
  }

  h1, h2, h3 {
    color: #333;
    margin-bottom: 20px;
  }

  .row {
    display: flex;
    gap: 10px;
    margin-bottom: 15px;
  }

  .ports-list {
    margin: 20px 0;
  }

  ul {
    list-style: none;
    padding: 0;
  }

  li {
    margin: 5px 0;
  }

  .port-select {
    width: 100%;
    text-align: left;
    padding: 8px;
    color: black;
    background: white;
    border: 1px solid #ddd;
    border-radius: 4px;
    cursor: pointer;
  }

  .port-select.selected {
    background: #e3f2fd;
    border-color: #2196f3;
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 15px;
    margin-bottom: 20px;
  }

  .setting {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  label {
    font-weight: bold;
    color: #555;
  }

  input, select {
    padding: 8px;
    border: 1px solid #ddd;
    border-radius: 4px;
    font-size: 14px;
  }

  button {
    padding: 8px 16px;
    background: #2196f3;
    color: white;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: 14px;
    transition: background 0.2s;
  }

  button:hover:not(:disabled) {
    background: #1976d2;
  }

  button:disabled {
    background: #ccc;
    cursor: not-allowed;
  }

  .status-info {
    display: flex;
    gap: 20px;
    margin: 15px 0;
    font-family: monospace;
  }

  .received-data {
    margin-top: 20px;
  }

  pre {
    background: #f5f5f5;
    padding: 15px;
    border-radius: 4px;
    overflow-x: auto;
    max-height: 200px;
    font-family: monospace;
  }

  .signals-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 15px;
  }

  .signal {
    text-align: center;
  }

  .indicator {
    padding: 8px;
    background: #f5f5f5;
    border-radius: 4px;
    font-family: monospace;
  }

  .active {
    background: #4caf50;
    color: white;
  }

  button.active {
    background: #4caf50;
  }
</style>
