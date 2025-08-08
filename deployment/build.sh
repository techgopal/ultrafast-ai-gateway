#!/bin/bash

# Ultrafast Gateway Docker Build Script
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default values
IMAGE_NAME="ultrafast-gateway"
TAG="latest"
BUILD_TYPE="release"

# Function to print colored output
print_status() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Function to show usage
show_usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -t, --tag TAG         Docker image tag (default: latest)"
    echo "  -b, --build-type TYPE  Build type: debug or release (default: release)"
    echo "  -p, --push            Push image to registry after build"
    echo "  -r, --run             Run container after build"
    echo "  -c, --compose         Use docker-compose instead of docker build"
    echo "  -h, --help            Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                    # Build release image with latest tag"
    echo "  $0 -t v1.0.0         # Build with specific tag"
    echo "  $0 -r                # Build and run container"
    echo "  $0 -c                # Use docker-compose"
    echo "  $0 -c -r             # Use docker-compose and run"
}

# Parse command line arguments
PUSH=false
RUN_AFTER_BUILD=false
USE_COMPOSE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -t|--tag)
            TAG="$2"
            shift 2
            ;;
        -b|--build-type)
            BUILD_TYPE="$2"
            shift 2
            ;;
        -p|--push)
            PUSH=true
            shift
            ;;
        -r|--run)
            RUN_AFTER_BUILD=true
            shift
            ;;
        -c|--compose)
            USE_COMPOSE=true
            shift
            ;;
        -h|--help)
            show_usage
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            show_usage
            exit 1
            ;;
    esac
done

# Validate build type
if [[ "$BUILD_TYPE" != "debug" && "$BUILD_TYPE" != "release" ]]; then
    print_error "Invalid build type: $BUILD_TYPE. Use 'debug' or 'release'"
    exit 1
fi

print_status "Building Ultrafast Gateway Docker image..."
print_status "Image: $IMAGE_NAME:$TAG"
print_status "Build type: $BUILD_TYPE"

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    print_error "Docker is not running. Please start Docker and try again."
    exit 1
fi

# Check if required files exist
if [[ ! -f "../Cargo.toml" ]]; then
    print_error "Cargo.toml not found in parent directory. Please ensure you're in the deployment directory."
    exit 1
fi

if [[ ! -f "env.development" ]]; then
    print_warning "env.development not found. Creating default environment file..."
    cat > env.development << 'EOF'
# Development Environment Configuration
DEVELOPMENT_MODE=true
RUST_LOG=info
RUST_BACKTRACE=1
REDIS_URL=redis://redis:6379
GATEWAY_JWT_SECRET=ultrafast-gateway-dev-secret-key
EOF
fi

# Build the image
if [[ "$USE_COMPOSE" == true ]]; then
    print_status "Using docker-compose to build..."
    docker-compose build gateway
    print_status "Build completed successfully!"
    
    if [[ "$RUN_AFTER_BUILD" == true ]]; then
        print_status "Starting services with docker-compose..."
        docker-compose up -d
        print_status "Services started. Gateway available at http://localhost:3000"
        print_status "Redis available at localhost:6379"
        print_status "Use 'docker-compose logs -f gateway' to view logs"
    fi
else
    print_status "Building with Docker..."
    docker build -t "$IMAGE_NAME:$TAG" .
    
    if [[ $? -eq 0 ]]; then
        print_status "Build completed successfully!"
        
        if [[ "$PUSH" == true ]]; then
            print_status "Pushing image to registry..."
            docker push "$IMAGE_NAME:$TAG"
        fi
        
        if [[ "$RUN_AFTER_BUILD" == true ]]; then
            print_status "Running container..."
            docker run -d \
                --name ultrafast-gateway \
                -p 3000:3000 \
                -e RUST_LOG=info \
                -e RUST_BACKTRACE=1 \
                "$IMAGE_NAME:$TAG"
            
            print_status "Container started. Gateway available at http://localhost:3000"
            print_status "Use 'docker logs ultrafast-gateway' to view logs"
        fi
    else
        print_error "Build failed!"
        exit 1
    fi
fi

print_status "Done!"
