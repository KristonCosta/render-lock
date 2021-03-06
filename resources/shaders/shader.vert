#version 450

layout(location=0) in vec3 a_position;
layout(location=1) in vec2 a_tex_coords;
layout(location=2) in vec3 a_normal;

layout(location=5) in vec4 model_matrix0;
layout(location=6) in vec4 model_matrix1;
layout(location=7) in vec4 model_matrix2;
layout(location=8) in vec4 model_matrix3;

layout(location=0) out vec2 v_tex_coords;
layout(location=1) out vec3 v_normal;
layout(location=2) out vec3 v_position;

layout(set=1, binding=0) uniform Uniforms {
    vec3 u_view_position;
    mat4 u_view_proj;
};

void main() {
    mat4 model_matrix = mat4(model_matrix0, model_matrix1, model_matrix2, model_matrix3);
    mat3 normal_matrix = mat3(transpose(inverse(model_matrix)));
    vec4 model_space = model_matrix * vec4(a_position, 1.0);
    v_tex_coords = a_tex_coords;
    v_normal = normal_matrix * a_normal;
    v_position = model_space.xyz;

    gl_Position = u_view_proj * model_space;
}