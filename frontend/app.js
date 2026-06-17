const API_URL = 'http://localhost:3000';
const WS_URL = 'ws://localhost:3000/ws';

let scene, camera, renderer, guibiaoGroup, sunLight, sunLightHelper;
let shadowMesh, gaugeMesh, rulerMesh;
let particles = [];
let showParticles = true;
let showLabels = true;
let currentMeasurement = null;
let currentSimulation = null;
let ws = null;

const CHI_SCALE = 0.5;
const GAUGE_HEIGHT_CHI = 40;
const RULER_LENGTH_CHI = 120;

function initThreeJS() {
    const container = document.getElementById('three-container');
    const width = container.clientWidth;
    const height = container.clientHeight;

    scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0a0a1a);
    scene.fog = new THREE.Fog(0x0a0a1a, 80, 200);

    camera = new THREE.PerspectiveCamera(50, width / height, 0.1, 1000);
    camera.position.set(60, 50, 80);
    camera.lookAt(0, 10, 0);

    renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setSize(width, height);
    renderer.setPixelRatio(window.devicePixelRatio);
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFSoftShadowMap;
    container.insertBefore(renderer.domElement, container.firstChild);

    const ambientLight = new THREE.AmbientLight(0x404050, 0.4);
    scene.add(ambientLight);

    const hemiLight = new THREE.HemisphereLight(0x87ceeb, 0x443322, 0.3);
    scene.add(hemiLight);

    sunLight = new THREE.DirectionalLight(0xfff5d4, 1.5);
    sunLight.position.set(50, 60, -30);
    sunLight.castShadow = true;
    sunLight.shadow.mapSize.width = 2048;
    sunLight.shadow.mapSize.height = 2048;
    sunLight.shadow.camera.near = 0.5;
    sunLight.shadow.camera.far = 300;
    sunLight.shadow.camera.left = -100;
    sunLight.shadow.camera.right = 100;
    sunLight.shadow.camera.top = 100;
    sunLight.shadow.camera.bottom = -100;
    scene.add(sunLight);

    createGround();
    createGuibiao();
    createSunParticles();

    window.addEventListener('resize', onWindowResize);
    animate();
}

function createGround() {
    const groundGeo = new THREE.PlaneGeometry(300, 300, 50, 50);
    const positions = groundGeo.attributes.position;
    for (let i = 0; i < positions.count; i++) {
        const x = positions.getX(i);
        const y = positions.getY(i);
        const noise = Math.sin(x * 0.05) * Math.cos(y * 0.05) * 0.3;
        positions.setZ(i, noise);
    }
    groundGeo.computeVertexNormals();
    const groundMat = new THREE.MeshStandardMaterial({
        color: 0x5a4a3a,
        roughness: 0.9,
        metalness: 0.1,
    });
    const ground = new THREE.Mesh(groundGeo, groundMat);
    ground.rotation.x = -Math.PI / 2;
    ground.receiveShadow = true;
    scene.add(ground);

    const gridHelper = new THREE.GridHelper(200, 40, 0x444466, 0x222233);
    gridHelper.position.y = 0.01;
    scene.add(gridHelper);
}

function createGuibiao() {
    guibiaoGroup = new THREE.Group();

    const baseGeo = new THREE.BoxGeometry(20, 2, 12);
    const baseMat = new THREE.MeshStandardMaterial({
        color: 0x6b5b4b,
        roughness: 0.8,
        metalness: 0.2,
    });
    const base = new THREE.Mesh(baseGeo, baseMat);
    base.position.y = 1;
    base.castShadow = true;
    base.receiveShadow = true;
    guibiaoGroup.add(base);

    const gaugeGeo = new THREE.BoxGeometry(1.5, GAUGE_HEIGHT_CHI * CHI_SCALE, 1.5);
    const gaugeMat = new THREE.MeshStandardMaterial({
        color: 0xc9a959,
        roughness: 0.4,
        metalness: 0.7,
    });
    gaugeMesh = new THREE.Mesh(gaugeGeo, gaugeMat);
    gaugeMesh.position.set(0, GAUGE_HEIGHT_CHI * CHI_SCALE / 2 + 2, 0);
    gaugeMesh.castShadow = true;
    guibiaoGroup.add(gaugeMesh);

    const topGeo = new THREE.BoxGeometry(3, 1, 3);
    const top = new THREE.Mesh(topGeo, gaugeMat);
    top.position.set(0, GAUGE_HEIGHT_CHI * CHI_SCALE + 2.5, 0);
    top.castShadow = true;
    guibiaoGroup.add(top);

    const rulerBaseGeo = new THREE.BoxGeometry(RULER_LENGTH_CHI * CHI_SCALE + 10, 0.8, 8);
    const rulerBaseMat = new THREE.MeshStandardMaterial({
        color: 0x5a4a3a,
        roughness: 0.9,
    });
    const rulerBase = new THREE.Mesh(rulerBaseGeo, rulerBaseMat);
    rulerBase.position.set(RULER_LENGTH_CHI * CHI_SCALE / 2 - 5, 0.4, 0);
    rulerBase.receiveShadow = true;
    guibiaoGroup.add(rulerBase);

    const rulerGeo = new THREE.BoxGeometry(RULER_LENGTH_CHI * CHI_SCALE, 0.3, 6);
    const rulerMat = new THREE.MeshStandardMaterial({
        color: 0xd4c4a4,
        roughness: 0.7,
    });
    rulerMesh = new THREE.Mesh(rulerGeo, rulerMat);
    rulerMesh.position.set(RULER_LENGTH_CHI * CHI_SCALE / 2 - 5, 1, 0);
    rulerMesh.receiveShadow = true;
    guibiaoGroup.add(rulerMesh);

    for (let i = 0; i <= RULER_LENGTH_CHI; i++) {
        const isMajor = i % 10 === 0;
        const tickHeight = isMajor ? 0.8 : 0.4;
        const tickGeo = new THREE.BoxGeometry(0.1, tickHeight, 0.1);
        const tickMat = new THREE.MeshBasicMaterial({ color: isMajor ? 0x000000 : 0x333333 });
        const tick = new THREE.Mesh(tickGeo, tickMat);
        tick.position.set(i * CHI_SCALE - 5, 1.3, 2.8);
        guibiaoGroup.add(tick);
    }

    const shadowGeo = new THREE.PlaneGeometry(0.01, 6);
    const shadowMat = new THREE.MeshBasicMaterial({
        color: 0x111111,
        transparent: true,
        opacity: 0.6,
        side: THREE.DoubleSide,
    });
    shadowMesh = new THREE.Mesh(shadowGeo, shadowMat);
    shadowMesh.rotation.x = -Math.PI / 2;
    shadowMesh.rotation.y = -Math.PI / 2;
    shadowMesh.position.set(GAUGE_HEIGHT_CHI * CHI_SCALE / 2, 1.16, 0);
    guibiaoGroup.add(shadowMesh);

    scene.add(guibiaoGroup);
}

function createSunParticles() {
    const particleCount = 200;
    const particleGeo = new THREE.BufferGeometry();
    const positions = new Float32Array(particleCount * 3);
    const colors = new Float32Array(particleCount * 3);
    const velocities = [];

    for (let i = 0; i < particleCount; i++) {
        positions[i * 3] = (Math.random() - 0.5) * 100;
        positions[i * 3 + 1] = 50 + Math.random() * 50;
        positions[i * 3 + 2] = -40 + (Math.random() - 0.5) * 30;

        colors[i * 3] = 1.0;
        colors[i * 3 + 1] = 0.9 + Math.random() * 0.1;
        colors[i * 3 + 2] = 0.6 + Math.random() * 0.2;

        velocities.push({
            x: 0,
            y: -0.1 - Math.random() * 0.2,
            z: 0.05 + Math.random() * 0.1,
        });
    }

    particleGeo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    particleGeo.setAttribute('color', new THREE.BufferAttribute(colors, 3));

    const particleMat = new THREE.PointsMaterial({
        size: 0.5,
        vertexColors: true,
        transparent: true,
        opacity: 0.8,
        blending: THREE.AdditiveBlending,
    });

    const particleSystem = new THREE.Points(particleGeo, particleMat);
    particleSystem.userData.velocities = velocities;
    particleSystem.userData.particleCount = particleCount;
    particles.push(particleSystem);
    scene.add(particleSystem);

    const beamGeo = new THREE.CylinderGeometry(0.3, 3, 80, 8, 1, true);
    const beamMat = new THREE.MeshBasicMaterial({
        color: 0xfff5d4,
        transparent: true,
        opacity: 0.08,
        side: THREE.DoubleSide,
    });
    const beam = new THREE.Mesh(beamGeo, beamMat);
    beam.position.set(15, 40, -15);
    beam.rotation.z = -0.5;
    beam.rotation.x = 0.3;
    guibiaoGroup.add(beam);
}

function updateSunPosition(altitudeDeg, azimuthDeg) {
    if (!sunLight) return;
    const alt = altitudeDeg * Math.PI / 180;
    const azi = azimuthDeg * Math.PI / 180;
    const distance = 100;
    const x = distance * Math.cos(alt) * Math.sin(azi);
    const y = distance * Math.sin(alt);
    const z = -distance * Math.cos(alt) * Math.cos(azi);
    sunLight.position.set(x, y, z);
    sunLight.intensity = Math.max(0.3, altitudeDeg / 60);
    if (particles.length > 0) {
        particles[0].visible = showParticles;
    }
}

function updateShadow(shadowLengthChi) {
    if (!shadowMesh) return;
    const len = Math.min(shadowLengthChi * CHI_SCALE, RULER_LENGTH_CHI * CHI_SCALE);
    shadowMesh.geometry.dispose();
    shadowMesh.geometry = new THREE.PlaneGeometry(Math.max(len, 0.1), 5);
    shadowMesh.position.x = Math.max(len / 2, 0.05);
}

function animate() {
    requestAnimationFrame(animate);

    if (showParticles && particles.length > 0) {
        const ps = particles[0];
        const positions = ps.geometry.attributes.position.array;
        const velocities = ps.userData.velocities;
        for (let i = 0; i < ps.userData.particleCount; i++) {
            positions[i * 3] += velocities[i].x;
            positions[i * 3 + 1] += velocities[i].y;
            positions[i * 3 + 2] += velocities[i].z;
            if (positions[i * 3 + 1] < 1) {
                positions[i * 3] = (Math.random() - 0.5) * 60;
                positions[i * 3 + 1] = 80 + Math.random() * 20;
                positions[i * 3 + 2] = -30 + (Math.random() - 0.5) * 20;
            }
        }
        ps.geometry.attributes.position.needsUpdate = true;
    }

    if (guibiaoGroup) {
        guibiaoGroup.rotation.y += 0.0003;
    }

    renderer.render(scene, camera);
}

function onWindowResize() {
    const container = document.getElementById('three-container');
    const width = container.clientWidth;
    const height = container.clientHeight;
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    renderer.setSize(width, height);
}

let shadowCtx;
let lightParticles = [];

function initShadowCanvas() {
    const canvas = document.getElementById('shadow-canvas');
    shadowCtx = canvas.getContext('2d');
    resizeShadowCanvas();
    window.addEventListener('resize', resizeShadowCanvas);

    for (let i = 0; i < 100; i++) {
        lightParticles.push({
            x: Math.random(),
            y: Math.random() * 0.3,
            speed: 0.002 + Math.random() * 0.004,
            size: 1 + Math.random() * 2,
            alpha: 0.3 + Math.random() * 0.4,
        });
    }

    drawShadowCanvas();
}

function resizeShadowCanvas() {
    const canvas = document.getElementById('shadow-canvas');
    const container = document.getElementById('shadow-canvas-container');
    canvas.width = container.clientWidth * window.devicePixelRatio;
    canvas.height = container.clientHeight * window.devicePixelRatio;
    canvas.style.width = container.clientWidth + 'px';
    canvas.style.height = container.clientHeight + 'px';
    shadowCtx.scale(window.devicePixelRatio, window.devicePixelRatio);
}

function drawShadowCanvas() {
    requestAnimationFrame(drawShadowCanvas);
    if (!shadowCtx) return;
    const w = document.getElementById('shadow-canvas-container').clientWidth;
    const h = document.getElementById('shadow-canvas-container').clientHeight;

    shadowCtx.clearRect(0, 0, w, h);

    const grad = shadowCtx.createLinearGradient(0, 0, 0, h);
    grad.addColorStop(0, 'rgba(30, 40, 70, 0.9)');
    grad.addColorStop(1, 'rgba(20, 30, 50, 0.95)');
    shadowCtx.fillStyle = grad;
    shadowCtx.fillRect(0, 0, w, h);

    const groundY = h * 0.75;
    const gaugeX = w * 0.15;
    const gaugeH = h * 0.6;
    const gaugeW = 12;

    shadowCtx.fillStyle = 'rgba(100, 80, 60, 0.3)';
    shadowCtx.fillRect(0, groundY, w, h - groundY);

    shadowCtx.strokeStyle = 'rgba(201, 169, 89, 0.3)';
    shadowCtx.lineWidth = 1;
    for (let i = 0; i < 20; i++) {
        const x = w * 0.2 + i * (w * 0.75 / 20);
        shadowCtx.beginPath();
        shadowCtx.moveTo(x, groundY);
        shadowCtx.lineTo(x, groundY + 8);
        shadowCtx.stroke();
        if (i % 5 === 0) {
            shadowCtx.fillStyle = 'rgba(201, 169, 89, 0.6)';
            shadowCtx.font = '10px Consolas';
            shadowCtx.fillText(i * 5 + '尺', x - 8, groundY + 22);
        }
    }

    if (showParticles) {
        const alt = currentMeasurement ? currentMeasurement.sun_altitude : 30;
        const azi = currentMeasurement ? currentMeasurement.sun_azimuth : 180;
        const dirX = Math.cos(alt * Math.PI / 180) * (azi > 180 ? 1 : -1);
        const dirY = -Math.sin(alt * Math.PI / 180);
        lightParticles.forEach(p => {
            p.x += dirX * p.speed;
            p.y += -dirY * p.speed * 0.5;
            if (p.x > 1 || p.y > 0.9) {
                p.x = 0.1 + Math.random() * 0.2;
                p.y = 0;
            }
            shadowCtx.beginPath();
            shadowCtx.arc(p.x * w, p.y * h, p.size, 0, Math.PI * 2);
            shadowCtx.fillStyle = `rgba(255, 240, 150, ${p.alpha})`;
            shadowCtx.fill();
        });
    }

    const gaugeGrad = shadowCtx.createLinearGradient(gaugeX - gaugeW / 2, groundY - gaugeH, gaugeX + gaugeW / 2, groundY);
    gaugeGrad.addColorStop(0, '#c9a959');
    gaugeGrad.addColorStop(0.5, '#d4b866');
    gaugeGrad.addColorStop(1, '#8b7333');
    shadowCtx.fillStyle = gaugeGrad;
    shadowCtx.fillRect(gaugeX - gaugeW / 2, groundY - gaugeH, gaugeW, gaugeH);

    shadowCtx.strokeStyle = '#000';
    shadowCtx.lineWidth = 1;
    for (let i = 0; i <= 40; i++) {
        const y = groundY - (gaugeH * i / 40);
        const ww = i % 5 === 0 ? 10 : 5;
        shadowCtx.beginPath();
        shadowCtx.moveTo(gaugeX - gaugeW / 2, y);
        shadowCtx.lineTo(gaugeX - gaugeW / 2 - ww, y);
        shadowCtx.stroke();
    }

    if (currentMeasurement) {
        const shadowLen = currentMeasurement.shadow_length;
        const shadowPx = (shadowLen / 100) * (w * 0.75);
        const shadowStartX = gaugeX + gaugeW / 2;
        const shadowEndX = Math.min(shadowStartX + shadowPx, w - 10);

        const shadowGrad = shadowCtx.createLinearGradient(shadowStartX, groundY, shadowEndX, groundY);
        shadowGrad.addColorStop(0, 'rgba(0, 0, 0, 0.85)');
        shadowGrad.addColorStop(0.5, 'rgba(0, 0, 0, 0.5)');
        shadowGrad.addColorStop(1, 'rgba(0, 0, 0, 0.15)');
        shadowCtx.fillStyle = shadowGrad;
        shadowCtx.fillRect(shadowStartX, groundY - 1, shadowEndX - shadowStartX, 25);

        const alt = currentMeasurement.sun_altitude;
        const topY = groundY - gaugeH;
        const rayLen = shadowEndX - gaugeX;
        const endY = groundY - gaugeH - rayLen * Math.tan(alt * Math.PI / 180);

        shadowCtx.strokeStyle = 'rgba(255, 220, 100, 0.4)';
        shadowCtx.lineWidth = 2;
        shadowCtx.setLineDash([5, 5]);
        shadowCtx.beginPath();
        shadowCtx.moveTo(gaugeX, topY);
        shadowCtx.lineTo(shadowEndX, groundY);
        shadowCtx.stroke();
        shadowCtx.setLineDash([]);

        if (showLabels) {
            shadowCtx.fillStyle = 'rgba(255, 220, 100, 0.9)';
            shadowCtx.font = 'bold 13px Consolas';
            shadowCtx.fillText(`影长: ${shadowLen.toFixed(2)} 尺 (${(shadowLen * 10).toFixed(1)} 寸)`, shadowStartX + 10, groundY - 8);
            shadowCtx.fillText(`太阳高度: ${alt.toFixed(2)}°`, shadowStartX + 10, groundY + 42);

            shadowCtx.beginPath();
            shadowCtx.moveTo(gaugeX, topY);
            shadowCtx.arc(gaugeX, topY, 30, Math.PI / 2, Math.PI / 2 + (90 - alt) * Math.PI / 180, false);
            shadowCtx.strokeStyle = 'rgba(201, 169, 89, 0.8)';
            shadowCtx.lineWidth = 2;
            shadowCtx.stroke();

            shadowCtx.fillStyle = '#c9a959';
            shadowCtx.font = 'bold 12px Consolas';
            shadowCtx.fillText(`${alt.toFixed(1)}°`, gaugeX + 35, topY + 15);

            shadowCtx.fillStyle = '#ff6b6b';
            shadowCtx.beginPath();
            shadowCtx.arc(shadowEndX, groundY, 5, 0, Math.PI * 2);
            shadowCtx.fill();

            shadowCtx.strokeStyle = '#ff6b6b';
            shadowCtx.lineWidth = 1;
            shadowCtx.beginPath();
            shadowCtx.moveTo(shadowEndX, groundY);
            shadowCtx.lineTo(shadowEndX, groundY - 35);
            shadowCtx.stroke();

            shadowCtx.fillStyle = '#ff6b6b';
            shadowCtx.font = 'bold 11px Consolas';
            shadowCtx.fillText(`影端`, shadowEndX - 12, groundY - 40);
        }
    }

    shadowCtx.fillStyle = 'rgba(201, 169, 89, 0.9)';
    shadowCtx.font = 'bold 11px Consolas';
    shadowCtx.fillText('圭(表高40尺)', gaugeX - 35, groundY - gaugeH - 10);
    shadowCtx.fillText('圭尺', w * 0.6, groundY + 38);
}

function updateMeasurementUI(m) {
    document.getElementById('m-time').textContent = formatTime(m.measurement_time);
    document.getElementById('m-gauge').innerHTML = `${m.gauge_height.toFixed(2)} <span class="data-unit">尺</span>`;
    document.getElementById('m-shadow').innerHTML = `${m.shadow_length.toFixed(2)} <span class="data-unit">尺</span>`;
    document.getElementById('m-shadow-cun').innerHTML = `${(m.shadow_length * 10).toFixed(1)} <span class="data-unit">寸</span>`;
    document.getElementById('m-alt').innerHTML = `${m.sun_altitude.toFixed(2)} <span class="data-unit">°</span>`;
    document.getElementById('m-azi').innerHTML = `${m.sun_azimuth.toFixed(1)} <span class="data-unit">°</span>`;
    document.getElementById('m-refr').textContent = m.atmospheric_refraction.toFixed(6);
    document.getElementById('m-tp').innerHTML = `${m.temperature.toFixed(1)}°C / ${m.pressure.toFixed(0)}hPa`;
    currentMeasurement = m;
    updateSunPosition(m.sun_altitude, m.sun_azimuth);
    updateShadow(m.shadow_length);
}

function updateSimulationUI(s) {
    document.getElementById('s-true-alt').textContent = s.true_sun_altitude.toFixed(4);
    document.getElementById('s-app-alt').textContent = s.apparent_sun_altitude.toFixed(4);
    document.getElementById('s-refr-corr').textContent = s.atmospheric_refraction_correction.toFixed(2);
    document.getElementById('s-curv-corr').textContent = s.earth_curvature_correction.toFixed(6);
    document.getElementById('s-theo-shadow').textContent = s.theoretical_shadow_length.toFixed(4);
    document.getElementById('s-refr-shadow').textContent = s.refracted_shadow_length.toFixed(4);
    const dev = s.shadow_deviation;
    const devEl = document.getElementById('s-deviation');
    devEl.textContent = dev.toFixed(3);
    devEl.style.color = Math.abs(dev) >= 1 ? '#ff6b6b' : '#c9a959';
    document.getElementById('s-solstice').textContent = s.winter_solstice_moment ? formatTime(s.winter_solstice_moment) : '非冬至期';
    currentSimulation = s;
}

function addAlert(alert) {
    const list = document.getElementById('alert-list');
    if (list.querySelector('div[style*="color: #666"]')) {
        list.innerHTML = '';
    }
    const level = alert.alert_level.toLowerCase();
    const item = document.createElement('div');
    item.className = `alert-item ${level}`;
    item.innerHTML = `
        <div class="alert-time">${formatTime(alert.alert_time)} | ${alert.alert_level}</div>
        <div class="alert-msg">${alert.message}</div>
    `;
    list.insertBefore(item, list.firstChild);
    while (list.children.length > 20) {
        list.removeChild(list.lastChild);
    }
}

function showMonteCarloResult(r) {
    document.getElementById('mc-result').style.display = 'block';
    document.getElementById('mc-count').textContent = r.simulation_count;
    document.getElementById('mc-shadow-std').textContent = r.shadow_length_std.toFixed(4);
    document.getElementById('mc-shadow-ci').textContent = `${r.shadow_length_95ci_low.toFixed(3)}, ${r.shadow_length_95ci_high.toFixed(3)}`;
    document.getElementById('mc-sol-std').textContent = r.solstice_time_std.toFixed(2);
    document.getElementById('mc-combined').textContent = r.combined_uncertainty.toFixed(6);
    document.getElementById('mc-expanded').textContent = r.expanded_uncertainty.toFixed(6);
}

function formatTime(isoStr) {
    if (!isoStr) return '--';
    try {
        const d = new Date(isoStr);
        return d.toLocaleString('zh-CN', { timeZone: 'Asia/Shanghai' });
    } catch {
        return isoStr;
    }
}

function connectWebSocket() {
    try {
        ws = new WebSocket(WS_URL);
    } catch (e) {
        console.error('WS连接失败:', e);
        setTimeout(connectWebSocket, 3000);
        return;
    }

    ws.onopen = () => {
        document.getElementById('ws-status').classList.add('connected');
        document.getElementById('ws-text').textContent = 'WebSocket: 已连接';
    };

    ws.onmessage = (event) => {
        try {
            const msg = JSON.parse(event.data);
            if (msg.message_type === 'measurement') {
                updateMeasurementUI(msg.data);
            } else if (msg.message_type === 'simulation') {
                updateSimulationUI(msg.data);
            } else if (msg.message_type === 'alert') {
                addAlert(msg.data);
            }
        } catch (e) {
            console.error('解析WS消息失败:', e);
        }
    };

    ws.onclose = () => {
        document.getElementById('ws-status').classList.remove('connected');
        document.getElementById('ws-text').textContent = 'WebSocket: 断开重连...';
        setTimeout(connectWebSocket, 3000);
    };

    ws.onerror = () => {
        if (ws) ws.close();
    };
}

async function loadInitialData() {
    try {
        const resp = await fetch(`${API_URL}/api/measurements/latest`);
        const result = await resp.json();
        if (result.success && result.data && result.data.length > 0) {
            updateMeasurementUI(result.data[0]);
        }
    } catch (e) {
        console.error('加载初始数据失败:', e);
    }
}

async function runMonteCarlo() {
    const btn = document.getElementById('btn-monte-carlo');
    btn.textContent = '分析中...';
    btn.disabled = true;
    try {
        const resp = await fetch(`${API_URL}/api/analyze/monte-carlo`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                simulation_count: 10000,
                gauge_height_error_std: 0.01,
                refraction_error_std: 5.0,
                confidence_level: 0.95,
            }),
        });
        const result = await resp.json();
        if (result.success) {
            showMonteCarloResult(result.data);
        }
    } catch (e) {
        console.error('蒙特卡洛分析失败:', e);
    }
    btn.textContent = '运行蒙特卡洛误差分析';
    btn.disabled = false;
}

function updateClock() {
    const now = new Date();
    document.getElementById('current-time').textContent = now.toLocaleString('zh-CN', { timeZone: 'Asia/Shanghai' });
}

document.addEventListener('DOMContentLoaded', () => {
    initThreeJS();
    initShadowCanvas();
    connectWebSocket();
    loadInitialData();
    setInterval(updateClock, 1000);
    updateClock();

    document.getElementById('btn-monte-carlo').addEventListener('click', runMonteCarlo);
    document.getElementById('toggle-particles').addEventListener('click', (e) => {
        showParticles = !showParticles;
        e.target.textContent = `粒子: ${showParticles ? '开' : '关'}`;
    });
    document.getElementById('toggle-labels').addEventListener('click', (e) => {
        showLabels = !showLabels;
        e.target.textContent = `标注: ${showLabels ? '开' : '关'}`;
    });
});
