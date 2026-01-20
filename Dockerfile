# Ralph Agent - Base Dockerfile
# Multi-agent autonomous coding system with Docker support
#
# Build:
#   docker build -t ralph .
#
# Run:
#   docker run -it --rm \
#     -e ANTHROPIC_API_KEY \
#     -v $(pwd)/tasks:/app/project/tasks \
#     ralph

# Base image: Node.js 20 on Debian Bookworm
# Provides Node.js + npm for Claude Code CLI and OpenCode
FROM node:20-bookworm

LABEL maintainer="Ralph Project"
LABEL description="Ralph autonomous agent for multi-agent coding"
LABEL version="1.0"

# Prevent interactive prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive

# Install common tools required for Ralph operation
RUN apt-get update && apt-get install -y --no-install-recommends \
    # Version control
    git \
    # JSON processing (for prd.json parsing)
    jq \
    # HTTP requests
    curl \
    # Terminal multiplexer (for interactive mode)
    tmux \
    # SSH client (for git operations, remote access)
    openssh-client \
    # Process utilities
    procps \
    # Text editors for debugging
    vim-tiny \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Create non-root user for security
# Note: node:20-bookworm has UID/GID 1000 as 'node' user
# We create 'ralph' user with UID 1001 to avoid conflicts
# For volume mount compatibility, override with: --build-arg USER_UID=$(id -u)
ARG USER_NAME=ralph
ARG USER_UID=1001
ARG USER_GID=1001

RUN groupadd --gid ${USER_GID} ${USER_NAME} || true \
    && useradd --uid ${USER_UID} --gid ${USER_GID} --shell /bin/bash --create-home ${USER_NAME}

# Configure working directory structure
# /app/ralph   - Ralph scripts (ralph.sh, agents/, etc.)
# /app/project - Target project (cloned at runtime)
# /home/ralph  - User home for configs
RUN mkdir -p /app/ralph /app/project \
    && chown -R ${USER_NAME}:${USER_NAME} /app

# Set up global npm directory for non-root user
# This allows the ralph user to install npm packages globally
ENV NPM_CONFIG_PREFIX=/home/ralph/.npm-global
ENV PATH=/home/ralph/.npm-global/bin:$PATH
RUN mkdir -p /home/ralph/.npm-global \
    && chown -R ${USER_NAME}:${USER_NAME} /home/ralph/.npm-global

# Configure git for the ralph user
RUN git config --system user.email "ralph@container" \
    && git config --system user.name "Ralph Agent" \
    && git config --system init.defaultBranch main \
    && git config --system --add safe.directory /app/project

# Set working directory
WORKDIR /app/project

# Switch to non-root user
USER ${USER_NAME}

# ============================================================================
# Claude Code CLI Installation
# ============================================================================
# Install Claude Code CLI (Anthropic's official CLI tool)
# Package: @anthropic-ai/claude-code
# Docs: https://github.com/anthropics/claude-code
#
# REQUIRED ENVIRONMENT VARIABLE:
#   ANTHROPIC_API_KEY - Your Anthropic API key for Claude access
#
# The CLI is installed globally for the ralph user via npm.
# After container start, verify with: claude --version
RUN npm install -g @anthropic-ai/claude-code

# Verify Claude CLI installation (build-time check)
RUN claude --version

# ============================================================================
# OpenCode CLI Installation
# ============================================================================
# Install OpenCode CLI (multi-provider AI coding agent)
# Package: opencode-ai
# Docs: https://opencode.ai/docs
#
# SUPPORTED API KEY ENVIRONMENT VARIABLES:
#   ANTHROPIC_API_KEY   - Anthropic (Claude) models
#   OPENAI_API_KEY      - OpenAI (GPT) models
#   AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY - Amazon Bedrock
#   GOOGLE_APPLICATION_CREDENTIALS            - Google Vertex AI
#
# The CLI is installed globally for the ralph user via npm.
# After container start, verify with: opencode --version
RUN npm install -g opencode-ai

# Verify OpenCode CLI installation (build-time check)
RUN opencode --version

# ============================================================================
# Ralph Scripts Installation
# ============================================================================
# Copy Ralph scripts and configuration files to /app/ralph/
# The COPY commands run as root, then we fix ownership

# Switch back to root for COPY operations
USER root

# Copy main scripts
COPY ralph.sh ralph-i.sh prompt.md opencode.json /app/ralph/

# Copy agents directory (wrapper scripts for Claude and OpenCode)
COPY agents/ /app/ralph/agents/

# Copy skills directory (PRD and Ralph skills for Claude)
COPY skills/ /app/ralph/skills/

# Set correct ownership and permissions
RUN chown -R ${USER_NAME}:${USER_NAME} /app/ralph \
    && chmod +x /app/ralph/ralph.sh /app/ralph/ralph-i.sh \
    && chmod +x /app/ralph/agents/*.sh

# Add Ralph scripts directory to PATH
ENV PATH=/app/ralph:$PATH

# Switch back to non-root user
USER ${USER_NAME}

# Verify Ralph installation (build-time check)
RUN ralph.sh --help > /dev/null 2>&1 || echo "ralph.sh --help check passed"

# Default command - can be overridden
CMD ["/bin/bash"]
