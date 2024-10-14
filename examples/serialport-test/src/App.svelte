<script lang="ts">
  import { SerialPort } from 'tauri-plugin-serialplugin';
  import { onMount } from 'svelte';

  let serialport: SerialPort | undefined = undefined;
  let name: string = '';
  let ports: { [key: string]: { type: string } } = {}; // Для хранения результата available_ports
  let directPorts: { [key: string]: { type: string } } = {}; // Для хранения результата available_ports_direct

  function openPort() {
    serialport = new SerialPort({ path: name, baudRate: 9600 });
    serialport
      .open()
      .then((res) => {
        console.log('open serialport', res);
      })
      .catch((err) => {
        console.error(err);
      });
  }

  function closePort() {
    if (serialport) {
      serialport
        .close()
        .then((res) => {
          console.log('close serialport', res);
        })
        .catch((err) => {
          console.error(err);
        });
    }
  }

  function available_ports() {
    SerialPort.available_ports()
      .then((res) => {
        console.log('available_ports: ', res);
        ports = res; // Сохраняем результат
      })
      .catch((err) => {
        console.error(err);
      });
  }

  function available_ports_direct() {
    SerialPort.available_ports_direct()
      .then((res) => {
        console.log('available_ports_direct: ', res);
        directPorts = res; // Сохраняем результат
      })
      .catch((err) => {
        console.error(err);
      });
  }
</script>

<main class="container">
  <h1>Welcome to Tauri Serial Port Plugin!</h1>

  <div class="row">
    <button on:click={available_ports}>Scan Ports</button>
    <button on:click={available_ports_direct}>Scan Ports Direct</button>
  </div>

  <!-- Список найденных портов -->
  <div class="ports-list">
    <h3>Available Ports</h3>
    {#if Object.keys(ports).length > 0}
      <ul>
        {#each Object.entries(ports) as [portName, info]}
          <li>
            <strong>{portName}:</strong> {info.type}
          </li>
        {/each}
      </ul>
    {:else}
      <p>No available ports found.</p>
    {/if}
  </div>

  <div class="ports-list">
    <h3>Available Ports (Direct)</h3>
    {#if Object.keys(directPorts).length > 0}
      <ul>
        {#each Object.entries(directPorts) as [portName, info]}
          <li>
            <strong>{portName}:</strong> {info.type}
          </li>
        {/each}
      </ul>
    {:else}
      <p>No available direct ports found.</p>
    {/if}
  </div>

  <!-- Подключение и отключение порта -->
  <div class="row">
    <input type="text" placeholder="write your com port here..." bind:value={name} />
    <button on:click={openPort}>Connect</button>
    <button on:click={closePort}>Disconnect</button>
  </div>
</main>

<style>
  .container {
    max-width: 800px;
    margin: 0 auto;
    padding: 20px;
    text-align: center;
  }

  .row {
    margin-bottom: 20px;
  }

  h1, h3 {
    margin-bottom: 10px;
    color: #333;
  }

  .ports-list {
    text-align: left;
    margin-bottom: 20px;
    padding: 15px;
    background-color: #f9f9f9;
    border: 1px solid #ddd;
    border-radius: 5px;
  }

  ul {
    list-style-type: disc;
    margin-left: 20px;
    padding-left: 10px;
  }

  li {
    margin: 5px 0;
    padding: 5px;
    background-color: #f0f0f0;
    border-radius: 5px;
    transition: background-color 0.2s ease;
  }

  li:hover {
    background-color: #e0e0e0;
  }

  input {
    padding: 10px;
    font-size: 14px;
    margin-right: 10px;
    border: 1px solid #ccc;
    border-radius: 5px;
    width: 60%;
  }

  button {
    padding: 10px 20px;
    font-size: 14px;
    background-color: #007bff;
    color: white;
    border: none;
    border-radius: 5px;
    cursor: pointer;
  }

  button:hover {
    background-color: #0056b3;
  }

  p {
    color: #666;
    font-style: italic;
  }
</style>
