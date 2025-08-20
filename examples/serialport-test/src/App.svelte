<script lang="ts">
  import { onMount } from "svelte";
  import { SerialPort } from "tauri-plugin-serialplugin-api";
  import { invoke } from "@tauri-apps/api/core";
  import SerialPortComponent from "./components/SerialPortComponent.svelte";

  // Lists of found ports
  let availablePorts: { [key: string]: { type: string } } = {};
  let directPorts: { [key: string]: { type: string } } = {};
  let managedPorts: string[] = [];

  // List of active connections
  let activePorts: string[] = [];

  // Field for manual path input
  let manualPath: string = "";

  // Scan ports
  async function scanPorts() {
    try {
      availablePorts = await SerialPort.available_ports();
      console.log("Available ports:", availablePorts);
    } catch (err) {
      console.error("Failed to scan ports:", err);
    }
  }

  async function scanPortsDirect() {
    try {
      directPorts = await SerialPort.available_ports_direct();
      console.log("Direct ports:", directPorts);
    } catch (err) {
      console.error("Failed to scan ports directly:", err);
    }
  }

  async function showManagedPorts() {
    try {
      managedPorts = await SerialPort.managed_ports();
      console.log("Managed ports:", managedPorts);
    } catch (err) {
      console.error("Failed to get managed ports:", err);
    }
  }

  // Call Rust command to get ports
  async function callRustGetPorts() {
    try {
      console.log("Calling Rust command to get ports...");
      const result = await invoke<string>("get_ports_programmatically");
      console.log("Rust command result:");
      console.log(result);
    } catch (err) {
      console.error("Error calling Rust command:", err);
    }
  }

  // Add port (on "Connect" click)
  function addPort(portName: string) {
    if (!portName) return;
    if (!activePorts.includes(portName)) {
      activePorts = [...activePorts, portName];
    }
  }

  // Remove port from active list (on disconnect event)
  function removePort(portName: string) {
    activePorts = activePorts.filter((name) => name !== portName);
  }

  // Click "Connect" for manual path
  function addManualPort() {
    const path = manualPath.trim();
    if (path) {
      addPort(path);
      manualPath = "";
    }
  }

  function handleDisconnect({ port }: { port: string }) {
    console.log("handleDisconnect triggered, port:", port);
    removePort(port);
  }

  // Scan everything on mount
  onMount(() => {
    scanPorts();
    scanPortsDirect();
    showManagedPorts();
  });
</script>

<main class="container">
  <header class="header">
    <h1 class="title">Serial Port Manager</h1>
    <p class="subtitle">Multi-port serial communication demo</p>
  </header>
  
  <!-- Rust Commands Section -->
  <section class="rust-command-section">
    <div class="section-header">
      <h2>🔧 Rust Commands</h2>
      <p>Test backend functionality</p>
    </div>
    <button on:click={callRustGetPorts} class="rust-button">
      <span class="icon">⚡</span>
      Get Ports via Rust Command
    </button>
    <p class="rust-hint">Result will be displayed in browser console</p>
  </section>

  <!-- Manual Port Input -->
  <section class="manual-connect">
    <div class="section-header">
      <h2>🔗 Manual Connection</h2>
      <p>Connect to a specific port</p>
    </div>
    <div class="input-group">
      <input
        type="text"
        placeholder="Enter port path (e.g. /dev/ttyACM0, COM1)..."
        bind:value={manualPath}
        class="port-input"
      />
      <button on:click={addManualPort} class="connect-btn">
        <span class="icon">➕</span>
        Connect
      </button>
    </div>
  </section>

  <!-- Port Lists Section -->
  <section class="scan-section">
    <div class="section-header">
      <h2>📡 Port Discovery</h2>
      <p>Available serial ports on your system</p>
    </div>
    <div class="port-lists">
      <!-- Available Ports -->
      <div class="port-group">
        <div class="group-header">
          <h3>🔍 Available Ports</h3>
          <button on:click={scanPorts} class="scan-btn">
            <span class="icon">🔄</span>
            Refresh
          </button>
        </div>
        {#if Object.keys(availablePorts).length > 0}
          <ul class="port-list">
            {#each Object.entries(availablePorts).sort( (a, b) => a[0].localeCompare(b[0]), ) as [portName, info]}
              <li class="port-item">
                <div class="port-info">
                  <span class="port-name">{portName}</span>
                  <span class="port-type">{info.type}</span>
                </div>
                <button on:click={() => addPort(portName)} class="add-btn">
                  <span class="icon">➕</span>
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="no-ports">No ports found</p>
        {/if}
      </div>

      <!-- Direct Ports -->
      <div class="port-group">
        <div class="group-header">
          <h3>🎯 Direct Ports</h3>
          <button on:click={scanPortsDirect} class="scan-btn">
            <span class="icon">🔄</span>
            Refresh
          </button>
        </div>
        {#if Object.keys(directPorts).length > 0}
          <ul class="port-list">
            {#each Object.entries(directPorts).sort( (a, b) => a[0].localeCompare(b[0]), ) as [portName, info]}
              <li class="port-item">
                <div class="port-info">
                  <span class="port-name">{portName}</span>
                  <span class="port-type">{info.type}</span>
                </div>
                <button on:click={() => addPort(portName)} class="add-btn">
                  <span class="icon">➕</span>
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="no-ports">No direct ports found</p>
        {/if}
      </div>

      <!-- Managed Ports -->
      <div class="port-group">
        <div class="group-header">
          <h3>⚙️ Managed Ports</h3>
          <button on:click={showManagedPorts} class="scan-btn">
            <span class="icon">🔄</span>
            Refresh
          </button>
        </div>
        {#if managedPorts.length > 0}
          <ul class="port-list">
            {#each managedPorts as portName}
              <li class="port-item">
                <div class="port-info">
                  <span class="port-name">{portName}</span>
                  <span class="port-type">Managed</span>
                </div>
                <button on:click={() => addPort(portName)} class="add-btn">
                  <span class="icon">➕</span>
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="no-ports">No managed ports found</p>
        {/if}
      </div>
    </div>
  </section>

  <!-- Active Connections -->
  <section class="active-ports">
    <div class="section-header">
      <h2>🔌 Active Connections</h2>
      <p>Currently connected serial ports</p>
    </div>
    {#if activePorts.length > 0}
      <div class="connections-grid">
        {#each activePorts as portName}
          <div class="port-wrapper">
            <SerialPortComponent {portName} onDisconnect={handleDisconnect} />
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-state">
        <div class="empty-icon">🔌</div>
        <p>No active connections</p>
        <p class="empty-hint">Add a port from the lists above to get started</p>
      </div>
    {/if}
  </section>
</main>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
    background: #f5f7fa;
    min-height: 100vh;
  }

  .container {
    max-width: 1400px;
    margin: 0 auto;
    padding: 20px;
    color: #333;
  }

  .header {
    text-align: center;
    margin-bottom: 40px;
    color: #2c3e50;
  }

  .title {
    font-size: 2.5rem;
    font-weight: 700;
    margin: 0 0 10px 0;
    color: #2c3e50;
  }

  .subtitle {
    font-size: 1.1rem;
    color: #7f8c8d;
    margin: 0;
    font-weight: 300;
  }

  section {
    margin-bottom: 30px;
    background: white;
    border-radius: 12px;
    padding: 24px;
    box-shadow: 0 4px 6px rgba(0,0,0,0.1);
    border: 1px solid rgba(255,255,255,0.2);
  }

  .section-header {
    margin-bottom: 20px;
  }

  .section-header h2 {
    margin: 0 0 8px 0;
    font-size: 1.5rem;
    font-weight: 600;
    color: #2c3e50;
  }

  .section-header p {
    margin: 0;
    color: #7f8c8d;
    font-size: 0.95rem;
  }

  /* Rust Commands */
  .rust-command-section {
    background: #667eea;
    color: white;
    border: none;
  }

  .rust-command-section .section-header h2,
  .rust-command-section .section-header p {
    color: white;
  }

  .rust-button {
    background: #ff6b6b;
    color: white;
    border: none;
    padding: 12px 24px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 1rem;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 8px;
    transition: all 0.3s ease;
    box-shadow: 0 2px 4px rgba(0,0,0,0.2);
  }

  .rust-button:hover {
    background: #ff5252;
    transform: translateY(-2px);
    box-shadow: 0 4px 8px rgba(0,0,0,0.3);
  }

  .rust-hint {
    margin-top: 12px;
    opacity: 0.8;
    font-size: 0.9rem;
  }

  /* Manual Connection */
  .input-group {
    display: flex;
    gap: 12px;
    align-items: center;
  }

  .port-input {
    flex: 1;
    padding: 12px 16px;
    border: 2px solid #e1e8ed;
    border-radius: 8px;
    font-size: 1rem;
    transition: border-color 0.3s ease;
  }

  .port-input:focus {
    outline: none;
    border-color: #667eea;
  }

  .connect-btn {
    background: #27ae60;
    color: white;
    border: none;
    padding: 12px 20px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 1rem;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 8px;
    transition: all 0.3s ease;
  }

  .connect-btn:hover {
    background: #229954;
    transform: translateY(-1px);
  }

  /* Port Lists */
  .port-lists {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
    gap: 24px;
  }

  .port-group {
    background: #f8f9fa;
    border: 1px solid #e9ecef;
    border-radius: 10px;
    padding: 20px;
  }

  .group-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .group-header h3 {
    margin: 0;
    font-size: 1.2rem;
    font-weight: 600;
    color: #2c3e50;
  }

  .scan-btn {
    background: #3498db;
    color: white;
    border: none;
    padding: 8px 16px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.9rem;
    display: flex;
    align-items: center;
    gap: 6px;
    transition: all 0.3s ease;
  }

  .scan-btn:hover {
    background: #2980b9;
  }

  .port-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .port-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px;
    margin: 8px 0;
    background: white;
    border-radius: 8px;
    border: 1px solid #e9ecef;
    transition: all 0.3s ease;
  }

  .port-item:hover {
    border-color: #667eea;
    box-shadow: 0 2px 4px rgba(0,0,0,0.1);
  }

  .port-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .port-name {
    font-weight: 600;
    color: #2c3e50;
  }

  .port-type {
    font-size: 0.85rem;
    color: #7f8c8d;
  }

  .add-btn {
    background: #27ae60;
    color: white;
    border: none;
    padding: 8px 12px;
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.3s ease;
  }

  .add-btn:hover {
    background: #229954;
    transform: scale(1.05);
  }

  .no-ports {
    text-align: center;
    color: #7f8c8d;
    font-style: italic;
    padding: 20px;
  }

  /* Active Connections */
  .connections-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(400px, 1fr));
    gap: 20px;
  }

  .port-wrapper {
    border: 1px solid #e9ecef;
    border-radius: 10px;
    padding: 20px;
    background: #f8f9fa;
  }

  .empty-state {
    text-align: center;
    padding: 40px 20px;
    color: #7f8c8d;
  }

  .empty-icon {
    font-size: 3rem;
    margin-bottom: 16px;
    opacity: 0.5;
  }

  .empty-hint {
    font-size: 0.9rem;
    margin-top: 8px;
    opacity: 0.7;
  }

  .icon {
    font-size: 1.1em;
    color: white;
  }

  /* Responsive Design */
  @media (max-width: 768px) {
    .container {
      padding: 15px;
    }

    .title {
      font-size: 2rem;
    }

    .port-lists {
      grid-template-columns: 1fr;
    }

    .input-group {
      flex-direction: column;
    }

    .connections-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
