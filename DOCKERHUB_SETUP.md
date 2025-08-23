# Docker Hub Publishing Setup

This guide explains how to set up Docker Hub publishing for the Ultrafast AI Gateway.

## 🐳 Overview

The CI workflow now publishes Docker images to both:
- **GitHub Container Registry (GHCR)**: `ghcr.io/techgopal/ultrafast-ai-gateway`
- **Docker Hub**: `techgopal/ultrafast-ai-gateway`

## 🔑 Required Secrets

You need to add these secrets to your GitHub repository:

### 1. Docker Hub Username
- **Secret Name**: `DOCKERHUB_USERNAME`
- **Value**: Your Docker Hub username (e.g., `techgopal`)

### 2. Docker Hub Access Token
- **Secret Name**: `DOCKERHUB_TOKEN`
- **Value**: Your Docker Hub access token (not your password!)

## 📋 Setup Steps

### Step 1: Create Docker Hub Account
1. Go to [Docker Hub](https://hub.docker.com)
2. Sign up or sign in to your account
3. Note your username

### Step 2: Create Docker Hub Access Token
1. Go to [Docker Hub Account Settings](https://hub.docker.com/settings/security)
2. Click "New Access Token"
3. Give it a name (e.g., "GitHub Actions")
4. Set permissions to "Read & Write"
5. Copy the generated token

### Step 3: Add Secrets to GitHub
1. Go to your GitHub repository
2. Click "Settings" → "Secrets and variables" → "Actions"
3. Click "New repository secret"
4. Add `DOCKERHUB_USERNAME` with your Docker Hub username
5. Add `DOCKERHUB_TOKEN` with your access token

## 🚀 How It Works

### Release Workflow
The workflow triggers when you:
- Push a tag (e.g., `v0.1.0`)
- Manually dispatch with a version

### What Gets Published
- **Crates**: `ultrafast-models-sdk` and `ultrafast-gateway` to crates.io
- **Docker Images**: 
  - `ghcr.io/techgopal/ultrafast-ai-gateway:latest`
  - `ghcr.io/techgopal/ultrafast-ai-gateway:v0.1.0`
  - `techgopal/ultrafast-ai-gateway:latest`
  - `techgopal/ultrafast-ai-gateway:v0.1.0`

### Multi-Platform Support
Images are built for:
- `linux/amd64` (x86_64)
- `linux/arm64` (ARM64)

## 📝 Docker Hub Repository Setup

### 1. Create Repository
1. Go to [Docker Hub](https://hub.docker.com)
2. Click "Create Repository"
3. Repository name: `ultrafast-ai-gateway`
4. Description: "High-performance AI gateway built in Rust"
5. Visibility: Public or Private (your choice)

### 2. Repository Settings
- **Short Description**: "High-performance AI gateway with advanced routing and caching"
- **Full Description**: Include usage examples and features
- **Documentation**: Link to your GitHub README

## 🔍 Testing Docker Hub Publishing

### Manual Test
```bash
# Test the workflow manually
# Go to Actions → Release → Run workflow
# Enter version: 0.1.0
```

### Verify Images
After successful publishing:
```bash
# Pull from Docker Hub
docker pull techgopal/ultrafast-ai-gateway:latest

# Run the image
docker run -p 3000:3000 techgopal/ultrafast-ai-gateway:latest

# Check health
curl http://localhost:3000/health
```

## 📊 Monitoring

### GitHub Actions
- Check the "Release" workflow in Actions tab
- Monitor build and push steps
- Check for any errors in Docker Hub authentication

### Docker Hub
- Monitor repository activity
- Check image tags and sizes
- Monitor pull statistics

## 🛠️ Troubleshooting

### Common Issues

#### 1. Authentication Failed
```
Error: failed to login to Docker Hub
```
**Solution**: Check `DOCKERHUB_TOKEN` secret value and permissions

#### 2. Repository Not Found
```
Error: repository techgopal/ultrafast-ai-gateway not found
```
**Solution**: Create the repository on Docker Hub first

#### 3. Permission Denied
```
Error: denied: requested access to the resource is denied
```
**Solution**: Ensure your Docker Hub token has "Read & Write" permissions

### Debug Steps
1. Check GitHub Actions logs
2. Verify secrets are correctly set
3. Test Docker Hub login manually
4. Check repository permissions

## 📚 Additional Resources

- [Docker Hub Documentation](https://docs.docker.com/docker-hub/)
- [GitHub Actions Docker Login](https://github.com/docker/login-action)
- [Multi-platform Docker Builds](https://docs.docker.com/build/building/multi-platform/)

## 🎯 Next Steps

1. ✅ Add Docker Hub secrets to GitHub
2. ✅ Create Docker Hub repository
3. ✅ Test with a manual release
4. ✅ Monitor first automated release
5. ✅ Update documentation with Docker Hub links

---

**Note**: The workflow will automatically publish to both registries when you create a new release tag!
