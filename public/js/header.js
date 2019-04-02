var HeaderEffect = (function(targetElem) {
  if(typeof window === 'undefined') {
    return {
      init: function(){},
      stop: function(){}
    };
  }
  var height, width, ctx, pointCount, points, oldTimestamp, doStop, doReset;
  var updateRate = 1/60 * 1000;
  function init() {
    var elem = document.getElementById(targetElem);

    height = elem.clientHeight;
    width = elem.clientWidth;

    elem.width = width;
    elem.height = height;

    ctx = elem.getContext('2d');

    pointCount = ((width * height) / 10000) | 0;
    if(pointCount < 5)
      pointCount = 5;
    if(pointCount > 100)
      pointCount = 100;
    points = [];
    for(var i =0; i < pointCount; i++) {
      points.push({
        id: i,
        x: (Math.random() * width) | 0,
        y: (Math.random() * height) | 0,
        radius: 2,
        xInc: ((Math.random() * 2) | 0) * 2 - 1,
        yInc: ((Math.random() * 2) | 0) * 2 - 1,
        xSpeed: Math.random() + 0.01,
        ySpeed: Math.random() + 0.01,
        joined: false,
      });
    }
    doStop = false;
    doReset = false;
    oldTimestamp = null;

    dettach();
    attach();
    draw();
  }

  function pointDistance(a, b) {
    var deltaX = a.x - b.x;
    var deltaY = a.y - b.y;
    deltaX *= deltaX;
    deltaY *= deltaY;
    return Math.sqrt(Math.abs(deltaX + deltaY));
  }

  function closestPoints(p) {
    return points.map(function(point) {
      return {id : point.id,
              x: point.x,
              y: point.y,
              d: pointDistance(p, point)};
    }).sort(function(a,b){
      return a.d - b.d;
    });
  }

  function joinPoints(a, b, soft) {
    if(b.joined == a.id)
      return;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
    if(!soft);
    a.joined = b.id;
  }
  var colors = [
    '#fff',
    '#f1f1f1',
    '#bbb',
    '#bbb'
  ];
  function draw(timestamp) {
    if(doStop) {
      return;
    }
    if(doReset) {
      init();
      return;
    }
    ctx.fillStyle = colors[0];
    ctx.fillRect(0,0,width, height);
    ctx.fillStyle = colors[2];
    for(var i = 0; i < pointCount; i++) {
      points[i].joined = false;
      var sortedArr = closestPoints(points[i]);
      ctx.strokeStyle = colors[3];
      joinPoints(points[i], sortedArr[1]);
      ctx.strokeStyle = colors[1];
      joinPoints(points[i], sortedArr[2], true);
      joinPoints(points[i], sortedArr[3], true);
      joinPoints(points[i], sortedArr[4], true);
    }
    ctx.strokeStyle = colors[3];
    for(var i = 0; i < pointCount; i++) {
      ctx.beginPath();
      ctx.arc(points[i].x, points[i].y, points[i].radius, 0, Math.PI*2);
      ctx.closePath();
      ctx.stroke();
      ctx.fill();
    }
    if(oldTimestamp == null) {
      update();
    } else {
      var elapsed = timestamp - oldTimestamp;

      while(elapsed > updateRate) {
        update();
        elapsed -= updateRate;
      }
      timestamp -= elapsed;
    }

    oldTimestamp = timestamp;
    window.requestAnimationFrame(draw);
  }

  function update() {
    for(var i = 0; i< pointCount; i++) {
      var point = points[i];
      var newXLocation = (point.xInc * point.xSpeed) + point.x;
      point.xInc = newXLocation < 0 || newXLocation > width ? point.xInc * - 1 : point.xInc;
      point.x = newXLocation;
      var newYLocation = (point.yInc * point.ySpeed) + point.y;
      point.yInc = newYLocation < 0 || newYLocation > height ? point.yInc * - 1 : point.yInc;
      point.y = newYLocation;
    }
  }

  function reset() {
    doReset = true;
  }

  function stop() {
    doStop = true;
    dettach();
  }

  function attachEventHandler(event, handler) {
    var oldHandler = window[event];
    if(oldHandler) {
      window[event] = function() {
        oldHandler.apply(this, arguments);
        handler.apply(this, arguments);
      };
    } else {
      window[event] = handler;
    }
  }

  var oldResize = window.onresize;
  function attach() {
    attachEventHandler('onresize', reset);
  }

  function dettach() {
    window.onresize = oldResize;
  }

  return {
    init: init,
    stop: stop,
    reset: reset,
  };
})('header-canvas');

HeaderEffect.init();
