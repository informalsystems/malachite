# Starknet Interoperability Setup

This setup generates networks composed of any combination of Malachite and Sequencer nodes. It uses a containerized architecture with configurable latency between nodes.

## Usage

1. Install dependencies:
    
    Run the appropriate script based on your OS from the `setup/` folder.

>[!NOTE]
> If you encounter an `error: externally-managed-environment` error while running the script, please refer to the last section of this document.

2. Clone the Starknet sequencer fork:
    ```bash
    git clone https://github.com/bastienfaivre/sequencer.git
    cd sequencer
    git checkout shahak/for_informalsystems/mock_batcher_to_return_empty_proposals
    ```

3. Generate a network:
    ```bash
    python3 generate.py --help
    ```

4. Apply the latency:
    ```bash
    ./apply-latency.sh <network name>
    ```

5. Spawn the network:
    ```bash
    ./manage.sh <network name> up
    ```

>[!NOTE]
> Spawning the network does not mean that the nodes have started yet. It only means that the containers are up and running. The `manage.sh` script outputs the commands to enter the containers if needed.

6. Build the nodes:
    
    If it is the first time you are running the nodes, or if you made changes to the code, you need to build the nodes. You can do this by running the following command:
    ```bash
    ./manage.sh <network name> build
    ```

7. Start the nodes:
    ```bash
    ./manage.sh <network name> start <duration in seconds>
    ```
    The logs are located in the `shared/networks/<network name>/logs/` folder.

8. Reset the state:

    You might want to reset the state (db, wal, etc.) of the nodes. You can do this by running the following command:
    ```bash
    ./manage.sh <network name> reset
    ```

9. Stop the network:
    ```bash
    ./manage.sh <network name> down
    ```

>[!NOTE]
> The `build`, `start`, and `reset` commands are also available inside each node's container. This can be useful if you want to run a command inside a specific node. Simply enter the container (with the command provided by the `manage.sh` script) and run the command: `build`, `start`, or `reset`.

## Advanced usage

You might want to make modifications to the config files of the nodes. You can find them under the `shared/networks/<network name>/<node name>/` folder. The config files are mounted in the containers so that you can edit them directly from your host machine.

>[!WARNING]
> Some Docker engines (e.g., Docker Desktop) sometimes have synchronization issues with mounted volumes, leading to corrupted config files in the containers. If you encounter such issues, stop the network, make your changes to the config files, and start the network again. It will ensure that the changes are applied correctly.

Moreover, for the latency, you can edit it in detail in the `shared/networks/<network name>/latencies.csv` file. Then, you can apply it again by running the `./apply-latency.sh <network name>` command. It will update the latency between the nodes without stopping the network. The `latencies.csv` file is a _from/to_ matrix, where the first column is the source node and the first row is the destination node. The values are the latencies in milliseconds. 

If you plan to make more long-term changes to the configuration, feel free to edit the templates under the `templates/` directory.

## Troubleshooting

### Error: `error: externally-managed-environment`

The easiest way to fix this error without using `--break-system-packages` is to create a global virtual environment imitating the system environment. It can be done by running the following setup:

```bash
# 1. Create a virtual environment
python3 -m venv ~/.venv
# 2. Create aliases to activate/deactivate the virtual environment anywhere
echo "alias py-start='source ~/.venv/bin/activate'" >> ~/.bashrc
echo "alias py-stop='deactivate'" >> ~/.bashrc
```

Then, you can activate the virtual environment by running `py-start` and deactivate it by running `py-stop`. You should now be able to install any package without any issues.
