# Malachite source code directory
MALACHITE_CODE_DIR?=$(realpath $(CURDIR)/../../code)

# Number of nodes for the testnet
NODES_COUNT?=5

# Malachite home directory
NODES_HOME?=$(HOME)/.malachite

# Binary name
APP_BINARY?=informalsystems-malachitebft-starknet-app

# Is this a RELEASE build?
ifeq ($(strip $(RELEASE)),)
BINARY_PATH=$(MALACHITE_CODE_DIR)/target/debug/$(APP_BINARY)
else
BINARY_PATH=$(MALACHITE_CODE_DIR)/target/release/$(APP_BINARY)
RELEASE_FLAG=--release
endif

CC=cargo

help:
	@echo "Commands:"
	@echo "  make               - show help"
	@echo "  make build         - build binary"
	@echo "  make -B build      - rebuild binary"
	@echo "  make testnet       - build configuration"
	@echo "  make -B testnet    - rebuild configuration"
	@echo "  make start / stop  - start/stop testnet in the background"
	@echo "  make log           - show log files' path"
	@echo "  make clean         - stop testnet and clean up all files"
	@echo "Environment variables:"
	@echo "  APP_BINARY         - the file name to run (informalsystems-malachitebft-starknet-app)"
	@echo "  MALACHITE_CODE_DIR - the source code directory ($(realpath $(CURDIR)/../../code))"
	@echo "  NODES_COUNT        - the number of local nodes to create (5)"
	@echo "  NODES_HOME         - home folder for the node configuration ($$HOME/.malachite)"
	@echo "  RELEASE            - if set, build a release version of the code (<unset> = debug version)"
.PHONY: help

###                       ###
### file creation targets ###
###                       ###

# Build the binary
$(BINARY_PATH):
	@cd $(MALACHITE_CODE_DIR) && $(CC) build $(RELEASE_FLAG) --package $(APP_BINARY)

# Create the configuration
$(NODES_HOME)/0:
	@$(MAKE) $(BINARY_PATH)
	@$(BINARY_PATH) testnet --home $(NODES_HOME) --nodes $(NODES_COUNT)


###                 ###
### manual commands ###
###                 ###


start:
	@$(MAKE) $(NODES_HOME)/0
	@for i in $$(seq 1 $(NODES_COUNT)); do \
	  o=$$(($$i - 1)) ;\
	  cd $(NODES_HOME)/$$o ;\
	  nohup "$(BINARY_PATH)" start --home $(NODES_HOME)/$$o & \
	done
.PHONY: start

stop:
	@killall -v -u $$UID $(APP_BINARY) 2> /dev/null || true
.PHONY: stop

build:
	@$(MAKE) $(BINARY_PATH)
.PHONY: build

testnet:
	@$(MAKE) $(NODES_HOME)/0
.PHONY: testnet

log:
	@for i in $$(seq 1 $(NODES_COUNT)); do \
	  o=$$(($$i - 1)) ;\
	  ls -lh $(NODES_HOME)/$$o/nohup.out 2> /dev/null || echo $(NODES_HOME)/$$o/nohup.out ;\
	done

clean:
	@$(MAKE) stop
	@test -n "$(NODES_HOME)" && test "$$(echo "$(NODES_HOME)" | wc -c)" -gt 2
	rm -rf $(NODES_HOME)


NODE?=4

restart: 
	pgrep -f "$(APP_BINARY).*$(NODES_HOME)/$(NODE)" | xargs -r kill -9
	sleep 1
	rm -rf $(NODES_HOME)/$(NODE)/db/*
	rm -rf $(NODES_HOME)/$(NODE)/wal/*
	rm -rf $(NODES_HOME)/$(NODE)/nohup.out.bak || true
	mv $(NODES_HOME)/$(NODE)/nohup.out $(NODES_HOME)/$(NODE)/nohup.out.bak || true
	cd $(NODES_HOME)/$(NODE) && nohup "$(BINARY_PATH)" start --home $(NODES_HOME)/$(NODE) &
.PHONY: restart

################################################################################
# Running a network with mixed versions of the sync protocol
################################################################################

MALACHITE_2_DIR?=$(CURDIR)/../../../malachite2
COMMIT2?=b55cfe9d # main 16/7/25

ifeq ($(strip $(RELEASE)),)
BINARY_PATH2=$(realpath $(MALACHITE_2_DIR))/code/target/debug/$(APP_BINARY)
else
BINARY_PATH2=$(realpath $(MALACHITE_2_DIR))/code/target/release/$(APP_BINARY)
RELEASE_FLAG=--release
endif

# Build a second binary from another commit
build-2:
	git clone git@github.com:informalsystems/malachite.git $(MALACHITE_2_DIR) || true
	cd $(MALACHITE_2_DIR) && git checkout $(COMMIT2)
	cd $(MALACHITE_2_DIR)/code && $(CC) build $(RELEASE_FLAG) --package $(APP_BINARY)
.PHONY= build-2

start-group-1:
	cd $(NODES_HOME)/0 ; nohup "$(BINARY_PATH)" start --home $(NODES_HOME)/0 &
	cd $(NODES_HOME)/1 ; nohup "$(BINARY_PATH)" start --home $(NODES_HOME)/1 &
	cd $(NODES_HOME)/2 ; nohup "$(BINARY_PATH)" start --home $(NODES_HOME)/2 &
	cd $(NODES_HOME)/3 ; nohup "$(BINARY_PATH)" start --home $(NODES_HOME)/3 &
.PHONY: start-group-1

start-group-2:
	cd $(NODES_HOME)/4 ; nohup "$(BINARY_PATH2)" start --home $(NODES_HOME)/4 &
.PHONY: start-group-2

# Start network with mixed versions
start-mixed: 
	@$(MAKE) start-group-1 
	sleep 120
	@$(MAKE) start-group-2
.PHONY: start-mixed

# Restart a node using the second binary
restart2: 
	pgrep -f "$(APP_BINARY).*$(NODES_HOME)/$(NODE)" | xargs -r kill -9
	sleep 1
	rm -rf $(NODES_HOME)/$(NODE)/db/*
	rm -rf $(NODES_HOME)/$(NODE)/wal/*
	rm -rf $(NODES_HOME)/$(NODE)/nohup.out.bak || true
	mv $(NODES_HOME)/$(NODE)/nohup.out $(NODES_HOME)/$(NODE)/nohup.out.bak || true
	cd $(NODES_HOME)/$(NODE) && nohup "$(BINARY_PATH2)" start --home $(NODES_HOME)/$(NODE) &
.PHONY: restart2

################################################################################
# Monitoring 
################################################################################

mon-up:
	docker compose -f monitoring/compose.yml up -d
	@echo Grafana dashboard is available at http://localhost:3000

mon-down:
	docker compose -f monitoring/compose.yml down

mon-logs:
	docker compose -f monitoring/compose.yml logs -f

.PHONY: mon-up mon-down mon-logs
